using System.IO.Compression;
using System.Text;
using System.Text.Json;
using System.Text.Json.Nodes;
using Cove.Connectors.Config;
using Cove.Connectors.Http;
using Cove.Connectors.Model;

namespace Cove.Connectors.Connectors;

/// <summary>
/// Install plans from GOG's content system (generation 2): the newest public
/// build, its depots (game, owned DLCs, matching language), every file with its
/// chunks and the CDN links for them. Native Linux games come as GOG's own
/// installer instead. Written from the format, not from other launchers' code.
/// </summary>
public sealed class GogContent(HttpClient http, GogConfig config)
{
    private const string Name = "GOG";

    private sealed record Build(string Id, string Version, string Link);

    public sealed record Depot(string ProductId, string Manifest);

    private static ConnectorException Unexpected(string what) =>
        new(ConnectorErrorKind.Unavailable, Texts.T($"GOG hat unerwartet geantwortet ({what}).", $"GOG answered unexpectedly ({what})."));

    private static string Token(StoreSession session) =>
        session.AccessToken ?? throw new ConnectorException(ConnectorErrorKind.AuthFailed, Texts.T("Bitte melde dich erneut bei GOG an.", "Please sign in to GOG again."));

    private static string CheckProduct(string productId) =>
        productId.Length is > 0 and < 20 && productId.All(char.IsAsciiDigit)
            ? productId
            : throw new ConnectorException(ConnectorErrorKind.InvalidRequest, Texts.T("Unbekanntes GOG-Spiel.", "Unknown GOG game."));

    private static string Fill(string template, string key, string value) =>
        template.Replace("{" + key + "}", value, StringComparison.Ordinal);

    /// <summary>"abcdef…" → "ab/cd/abcdef…" (how GOG's CDN stores manifests and chunks).</summary>
    public static string GalaxyPath(string hash) =>
        hash.Length < 4 ? throw Unexpected("hash") : $"{hash[..2]}/{hash[2..4]}/{hash}";

    /// <summary>Manifests are zlib-compressed JSON (older ones plain JSON).</summary>
    private async Task<JsonNode> MetaAsync(string url, CancellationToken ct)
    {
        byte[] bytes;
        try
        {
            bytes = await http.GetByteArrayAsync(new Uri(url), ct).ConfigureAwait(false);
        }
        catch (HttpRequestException e) when (e.StatusCode is not null)
        {
            throw new ConnectorException(ConnectorErrorKind.Unavailable, Texts.T(
                $"GOG ist gerade nicht verfügbar (HTTP {(int)e.StatusCode}).", $"GOG is not available right now (HTTP {(int)e.StatusCode})."), e);
        }
        catch (HttpRequestException e)
        {
            throw new ConnectorException(ConnectorErrorKind.Network, Texts.T("GOG ist nicht erreichbar.", "GOG can't be reached."), e);
        }

        try
        {
            if (bytes.Length > 0 && bytes[0] == (byte)'{')
            {
                return JsonNode.Parse(bytes) ?? throw Unexpected("manifest");
            }

            using var input = new MemoryStream(bytes);
            using var zlib = new ZLibStream(input, CompressionMode.Decompress);
            return JsonNode.Parse(zlib) ?? throw Unexpected("manifest");
        }
        catch (Exception e) when (e is JsonException or InvalidDataException)
        {
            throw new ConnectorException(ConnectorErrorKind.Unavailable, Texts.T("GOG hat ein beschädigtes Manifest geliefert.", "GOG sent a damaged manifest."), e);
        }
    }

    private async Task<Build> LatestBuildAsync(string token, string productId, string os, CancellationToken ct)
    {
        var url = Fill(Fill(config.BuildsUrl, "productId", productId), "os", os);
        var body = await StoreHttp.GetJsonAsync(http, url, Name, ct, token).ConfigureAwait(false);
        var items = body["items"]?.AsArray() ?? [];
        var publicBuilds = items.Where(b => b?["branch"] is null || StoreHttp.Optional(b, "branch") is null).ToList();
        var latest = publicBuilds.FirstOrDefault(b => StoreHttp.OptionalLong(b, "generation") == 2);
        if (latest is null)
        {
            throw publicBuilds.Count > 0
                ? new ConnectorException(ConnectorErrorKind.Unavailable, Texts.T(
                    "Für dieses Spiel bietet GOG nur das alte Download-Format an. Installiere es über GOG Galaxy.",
                    "GOG only offers the old download format for this game. Install it with GOG Galaxy."))
                : new ConnectorException(ConnectorErrorKind.Unavailable, Texts.T(
                    "GOG bietet für dieses Spiel keinen passenden Download an.",
                    "GOG offers no matching download for this game."));
        }

        return new Build(
            StoreHttp.Required(latest, "build_id", Name),
            StoreHttp.Optional(latest, "version_name") ?? string.Empty,
            StoreHttp.Required(latest, "link", Name));
    }

    private async Task<HashSet<string>> OwnedAsync(string token, CancellationToken ct)
    {
        var body = await StoreHttp.GetJsonAsync(http, config.OwnedIdsUrl, Name, ct, token).ConfigureAwait(false);
        return [.. (body["owned"]?.AsArray() ?? []).Select(n => n?.ToString() ?? string.Empty).Where(s => s.Length > 0)];
    }

    /// <summary>"de" matches "de-DE", "de" and "German"; "*" matches every language.</summary>
    public static bool SpeaksLanguage(IEnumerable<string> languages, string language)
    {
        var names = language == "de" ? new[] { "german", "deutsch" } : ["english"];
        return languages.Any(l =>
            l == "*" ||
            l.Equals(language, StringComparison.OrdinalIgnoreCase) ||
            l.StartsWith(language + "-", StringComparison.OrdinalIgnoreCase) ||
            names.Contains(l.ToLowerInvariant()));
    }

    private static List<string> Languages(JsonNode? depot) =>
        [.. (depot?["languages"]?.AsArray() ?? []).Select(n => n?.ToString() ?? string.Empty)];

    /// <summary>Depots of the game and owned DLCs in the user's language (English when the game has none), 64-bit when there is a choice.</summary>
    public static List<Depot> PickDepots(JsonNode manifest, string baseProduct, ISet<string> owned, string language)
    {
        var all = (manifest["depots"]?.AsArray() ?? []).Where(d => d is not null).Select(d => d!).ToList();
        if (manifest["offlineDepot"] is { } offline)
        {
            all.Add(offline);
        }

        var mine = all.Where(d =>
        {
            var product = StoreHttp.Optional(d, "productId") ?? baseProduct;
            return product == baseProduct || owned.Contains(product);
        }).ToList();
        var localized = mine.Any(d => Languages(d).Any(l => l != "*") && SpeaksLanguage(Languages(d).Where(l => l != "*"), language));
        var chosen = localized ? language : "en";
        var bitness = (JsonNode d) => (d["osBitness"]?.AsArray() ?? []).Select(n => n?.ToString()).ToList();
        var has64 = mine.Any(d => bitness(d).Contains("64"));
        return [.. mine
            .Where(d => SpeaksLanguage(Languages(d), chosen) || Languages(d).Count == 0)
            .Where(d => !has64 || bitness(d) is not ["32"])
            .Select(d => new Depot(StoreHttp.Optional(d, "productId") ?? baseProduct, StoreHttp.Required(d, "manifest", Name)))
            .DistinctBy(d => d.Manifest)];
    }

    /// <summary>DLCs among the planned products, named from the build manifest's product list.</summary>
    public static List<GogDlc> Dlcs(JsonNode manifest, string baseProduct, ISet<string> planned)
    {
        var names = (manifest["products"]?.AsArray() ?? [])
            .Where(p => StoreHttp.Optional(p, "productId") is not null)
            .GroupBy(p => StoreHttp.Optional(p, "productId")!, StringComparer.Ordinal)
            .ToDictionary(g => g.Key, g => StoreHttp.Optional(g.First(), "name"), StringComparer.Ordinal);
        return [.. planned
            .Where(p => p != baseProduct)
            .Order(StringComparer.Ordinal)
            .Select(p => new GogDlc(p, names.GetValueOrDefault(p) is { Length: > 0 } name ? name : p))];
    }

    private static List<GogChunk> Chunks(JsonNode? node) =>
        [.. (node?["chunks"]?.AsArray() ?? []).Select(c => new GogChunk(
            StoreHttp.Required(c, "md5", Name),
            StoreHttp.Required(c, "compressedMd5", Name),
            StoreHttp.OptionalLong(c, "size") ?? throw Unexpected("size"),
            StoreHttp.OptionalLong(c, "compressedSize") ?? throw Unexpected("compressedSize")))];

    /// <summary>Relative path with forward slashes; refuses anything that leaves the game folder.</summary>
    public static string CleanPath(string raw)
    {
        var path = raw.Replace('\\', '/').TrimStart('/');
        if (path.Length == 0 || path.Split('/').Any(part => part is ".." or "") || path.Contains(':', StringComparison.Ordinal))
        {
            throw Unexpected("path");
        }

        return path;
    }

    /// <summary>CDN URL templates for one product, best first (they carry a token and expire).</summary>
    private async Task<List<string>> ChunkUrlsAsync(string token, string productId, CancellationToken ct)
    {
        var body = await StoreHttp.GetJsonAsync(http, Fill(config.SecureLinkUrl, "productId", productId), Name, ct, token).ConfigureAwait(false);
        var urls = new List<(long Priority, string Url)>();
        foreach (var entry in body["urls"]?.AsArray() ?? [])
        {
            if (entry?["parameters"] is not JsonObject parameters || StoreHttp.Optional(entry, "url_format") is not { } format)
            {
                continue;
            }

            var url = format;
            foreach (var (key, value) in parameters)
            {
                var text = key == "path"
                    ? (value?.ToString() ?? string.Empty).TrimEnd('/') + "/{chunk}"
                    : value?.ToString() ?? string.Empty;
                url = Fill(url, key, text);
            }

            urls.Add((StoreHttp.OptionalLong(entry, "priority") ?? 0, url));
        }

        return urls.Count > 0
            ? [.. urls.OrderByDescending(u => u.Priority).Select(u => u.Url)]
            : throw Unexpected("secure_link");
    }

    /// <summary>GOG's own installer for native Linux games, or null when there is none.</summary>
    private async Task<GogInstallPlan?> LinuxInstallerAsync(string token, string productId, string language, CancellationToken ct)
    {
        var body = await StoreHttp.GetJsonAsync(http, Fill(config.ProductDownloadsUrl, "productId", productId), Name, ct, token).ConfigureAwait(false);
        var linux = (body["downloads"]?["installers"]?.AsArray() ?? [])
            .Where(i => StoreHttp.Optional(i, "os") == "linux" && i?["files"]?.AsArray().Count == 1)
            .ToList();
        var installer = linux.FirstOrDefault(i => StoreHttp.Optional(i, "language") == language)
            ?? linux.FirstOrDefault(i => StoreHttp.Optional(i, "language") == "en")
            ?? linux.FirstOrDefault();
        if (installer?["files"]?[0] is not { } file)
        {
            return null;
        }

        var link = await StoreHttp.GetJsonAsync(http, StoreHttp.Required(file, "downlink", Name), Name, ct, token).ConfigureAwait(false);
        var url = StoreHttp.Required(link, "downlink", Name);
        var fileName = Uri.UnescapeDataString(new Uri(url).AbsolutePath.Split('/')[^1]);
        var size = StoreHttp.OptionalLong(file, "size") ?? StoreHttp.OptionalLong(installer, "total_size") ?? 0;
        var version = StoreHttp.Optional(installer, "version") ?? string.Empty;
        return new GogInstallPlan(
            productId, "linux", $"installer:{version}", version, size, size, [], [], [], [], [], new GogInstaller(url, fileName, size), []);
    }

    /// <summary>
    /// DLCs of the game the user owns (GOG's product list), marked when no build
    /// of the game has files for them. Unknown builds (old format only) leave them unmarked.
    /// </summary>
    public async Task<OwnedDlcs> OwnedDlcsAsync(StoreSession session, string productId, string platform, CancellationToken ct)
    {
        var token = Token(session);
        CheckProduct(productId);
        var owned = await OwnedAsync(token, ct).ConfigureAwait(false);
        var product = await StoreHttp.GetJsonAsync(http, Fill(config.ProductDlcsUrl, "productId", productId), Name, ct).ConfigureAwait(false);
        var dlcs = (product["expanded_dlcs"]?.AsArray() ?? [])
            .Select(d => (Id: d?["id"]?.ToString() ?? string.Empty, Title: StoreHttp.Optional(d, "title")))
            .Where(d => d.Id.Length > 0 && owned.Contains(d.Id))
            .DistinctBy(d => d.Id)
            .ToList();
        if (dlcs.Count == 0)
        {
            return new OwnedDlcs([]);
        }

        var withFiles = await DepotProductsAsync(token, productId, platform, ct).ConfigureAwait(false);
        return new OwnedDlcs([.. dlcs.Select(d => new OwnedDlc(d.Id, d.Title ?? d.Id, withFiles is not null && !withFiles.Contains(d.Id)))]);
    }

    /// <summary>Products with files in the newest build (Windows build as fallback); null when GOG has none.</summary>
    private async Task<HashSet<string>?> DepotProductsAsync(string token, string productId, string platform, CancellationToken ct)
    {
        string[] systems = platform == "osx" ? ["osx", "windows"] : platform == "linux" ? ["linux", "windows"] : ["windows"];
        foreach (var os in systems)
        {
            try
            {
                var build = await LatestBuildAsync(token, productId, os, ct).ConfigureAwait(false);
                var manifest = await MetaAsync(build.Link, ct).ConfigureAwait(false);
                var baseProduct = StoreHttp.Optional(manifest, "baseProductId") ?? productId;
                return [.. (manifest["depots"]?.AsArray() ?? []).Select(d => StoreHttp.Optional(d, "productId") ?? baseProduct)];
            }
            catch (ConnectorException e) when (e.Kind is ConnectorErrorKind.Unavailable)
            {
                // No (new-format) build for this system: try the next one.
            }
        }

        return null;
    }

    public async Task<GogInstallPlan> PlanAsync(StoreSession session, string productId, string platform, string language, CancellationToken ct)
    {
        var token = Token(session);
        CheckProduct(productId);
        if (platform == "linux" && await LinuxInstallerAsync(token, productId, language, ct).ConfigureAwait(false) is { } installer)
        {
            return installer;
        }

        var build = await LatestBuildAsync(token, productId, platform == "osx" ? "osx" : "windows", ct).ConfigureAwait(false);
        var manifest = await MetaAsync(build.Link, ct).ConfigureAwait(false);
        if (StoreHttp.OptionalLong(manifest, "version") != 2)
        {
            throw Unexpected("manifest version");
        }

        var owned = await OwnedAsync(token, ct).ConfigureAwait(false);
        var baseProduct = StoreHttp.Optional(manifest, "baseProductId") ?? productId;
        var files = new List<GogFile>();
        var directories = new List<string>();
        var links = new List<GogLink>();
        var containers = new List<GogContainer>();
        var seen = new HashSet<string>(StringComparer.OrdinalIgnoreCase);
        foreach (var depot in PickDepots(manifest, baseProduct, owned, language))
        {
            var body = await MetaAsync(Fill(config.MetaUrl, "path", GalaxyPath(depot.Manifest)), ct).ConfigureAwait(false);
            var content = body["depot"] ?? throw Unexpected("depot");
            int? container = null;
            if (content["smallFilesContainer"] is { } sfc)
            {
                containers.Add(new GogContainer(depot.ProductId, Chunks(sfc)));
                container = containers.Count - 1;
            }

            foreach (var item in content["items"]?.AsArray() ?? [])
            {
                var path = CleanPath(StoreHttp.Required(item, "path", Name));
                switch (StoreHttp.Optional(item, "type"))
                {
                    case "DepotDirectory":
                        directories.Add(path);
                        break;
                    case "DepotLink":
                        links.Add(new GogLink(path, StoreHttp.Required(item, "target", Name).Replace('\\', '/')));
                        break;
                    case "DepotFile" when seen.Add(path):
                        var flags = (item?["flags"]?.AsArray() ?? []).Select(f => f?.ToString()).ToList();
                        var sfcRef = item?["sfcRef"] is { } r && container is { } index
                            ? new GogSfcRef(index, StoreHttp.OptionalLong(r, "offset") ?? 0, StoreHttp.OptionalLong(r, "size") ?? 0)
                            : null;
                        files.Add(new GogFile(path, depot.ProductId, sfcRef is null ? Chunks(item) : [], flags.Contains("executable"), sfcRef));
                        break;
                }
            }
        }

        var products = files.Select(f => f.ProductId).Concat(containers.Select(c => c.ProductId)).ToHashSet();
        var urls = new Dictionary<string, List<string>>();
        foreach (var product in products)
        {
            urls[product] = await ChunkUrlsAsync(token, product, ct).ConfigureAwait(false);
        }

        var chunks = files.SelectMany(f => f.Chunks).Concat(containers.SelectMany(c => c.Chunks)).ToList();
        var installBytes = files.Sum(f => f.Sfc?.Size ?? f.Chunks.Sum(c => c.Size));
        return new GogInstallPlan(
            productId, platform == "osx" ? "osx" : "windows", build.Id, build.Version,
            chunks.Sum(c => c.CompressedSize), installBytes, directories, files, links, containers, urls, null,
            Dlcs(manifest, baseProduct, products));
    }

    /// <summary>The newest public build or installer version, for update checks.</summary>
    public async Task<GogLatest> LatestAsync(StoreSession session, string productId, string platform, string language, CancellationToken ct)
    {
        var token = Token(session);
        CheckProduct(productId);
        if (platform == "linux")
        {
            var body = await StoreHttp.GetJsonAsync(http, Fill(config.ProductDownloadsUrl, "productId", productId), Name, ct, token).ConfigureAwait(false);
            var installer = (body["downloads"]?["installers"]?.AsArray() ?? [])
                .Where(i => StoreHttp.Optional(i, "os") == "linux" && i?["files"]?.AsArray().Count == 1)
                .OrderByDescending(i => StoreHttp.Optional(i, "language") == language)
                .FirstOrDefault();
            if (installer is not null)
            {
                var version = StoreHttp.Optional(installer, "version") ?? string.Empty;
                return new GogLatest($"installer:{version}", version, "linux");
            }
        }

        var os = platform == "osx" ? "osx" : "windows";
        var build = await LatestBuildAsync(token, productId, os, ct).ConfigureAwait(false);
        return new GogLatest(build.Id, build.Version, os);
    }

    /// <summary>Zlib-compresses JSON like GOG's CDN (test fixtures).</summary>
    public static byte[] Compress(string json)
    {
        using var output = new MemoryStream();
        using (var zlib = new ZLibStream(output, CompressionLevel.Fastest))
        {
            zlib.Write(Encoding.UTF8.GetBytes(json));
        }

        return output.ToArray();
    }
}
