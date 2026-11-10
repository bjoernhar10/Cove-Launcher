using System.Net.Http.Headers;
using System.Text.Json;
using System.Text.Json.Nodes;
using Cove.Connectors.Config;
using Cove.Connectors.Http;
using Cove.Connectors.Model;

namespace Cove.Connectors.Connectors;

/// <summary>
/// The session-bound parts of Epic installs: manifest links, catalogue
/// attributes, exchange codes and ownership tokens for starting a game, and
/// the cloud-save storage. Written from Epic's public endpoints, not from
/// other launchers' code.
/// </summary>
public sealed class EpicContent(HttpClient http, EpicConfig config)
{
    private const string Name = "Epic Games";

    private static string Token(StoreSession session) =>
        session.AccessToken ?? throw new ConnectorException(ConnectorErrorKind.AuthFailed, Texts.T("Bitte melde dich erneut bei Epic an.", "Please sign in to Epic again."));

    private static string Fill(string template, params (string Key, string Value)[] values) =>
        values.Aggregate(template, (url, v) => url.Replace("{" + v.Key + "}", Uri.EscapeDataString(v.Value), StringComparison.Ordinal));

    private static bool IsAppName(string appName) =>
        appName.Length is > 0 and < 128 && appName.All(c => char.IsAsciiLetterOrDigit(c) || c is '-' or '_' or '.');

    private static string CheckApp(string appName) =>
        IsAppName(appName)
            ? appName
            : throw new ConnectorException(ConnectorErrorKind.InvalidRequest, Texts.T("Unbekanntes Epic-Spiel.", "Unknown Epic game."));

    /// <summary>Everything in the account's library: namespace, catalogue item and app name per record.</summary>
    private async Task<List<(string Namespace, string CatalogItemId, string AppName)>> RecordsAsync(StoreSession session, CancellationToken ct)
    {
        var records = new List<(string, string, string)>();
        string? cursor = null;
        for (var i = 0; i < 100; i++)
        {
            var url = cursor is null ? config.LibraryUrl : $"{config.LibraryUrl}&cursor={Uri.EscapeDataString(cursor)}";
            var page = await StoreHttp.GetJsonAsync(http, url, Name, ct, Token(session)).ConfigureAwait(false);
            foreach (var record in page["records"]?.AsArray() ?? [])
            {
                if (StoreHttp.Optional(record, "namespace") is { } ns && StoreHttp.Optional(record, "catalogItemId") is { } item)
                {
                    records.Add((ns, item, StoreHttp.Optional(record, "appName") ?? string.Empty));
                }
            }

            cursor = StoreHttp.Optional(page["responseMetadata"], "nextCursor");
            if (string.IsNullOrEmpty(cursor))
            {
                break;
            }
        }

        return records;
    }

    /// <summary>Namespace and catalogue item of an owned game (the library only keeps the app name).</summary>
    private static (string Namespace, string CatalogItemId) Find(List<(string Namespace, string CatalogItemId, string AppName)> records, string appName)
    {
        foreach (var (ns, item, app) in records)
        {
            if (app == appName)
            {
                return (ns, item);
            }
        }

        throw new ConnectorException(ConnectorErrorKind.InvalidRequest, Texts.T(
            "Dieses Spiel gehört nicht (mehr) zu deinem Epic-Konto.", "This game is not (or no longer) in your Epic account."));
    }

    /// <summary>"Value" of a catalogue custom attribute.</summary>
    private static string? Attribute(JsonNode? item, string key) => StoreHttp.Optional(item?["customAttributes"]?[key], "value");

    /// <summary>Build version and signed manifest links of one app, or null when Epic has no build for this platform.</summary>
    private async Task<(string BuildVersion, List<string> ManifestUrls)?> ManifestsAsync(
        StoreSession session, string epicPlatform, string ns, string item, string appName, CancellationToken ct)
    {
        var assets = await StoreHttp.GetJsonOrNullAsync(
            http,
            Fill(config.AssetsUrl, ("platform", epicPlatform), ("namespace", ns), ("catalogItemId", item), ("appName", appName)),
            Name, ct, Token(session)).ConfigureAwait(false);
        if (assets?["elements"]?.AsArray().FirstOrDefault() is not { } element)
        {
            return null;
        }

        var manifests = new List<string>();
        foreach (var manifest in element["manifests"]?.AsArray() ?? [])
        {
            if (StoreHttp.Optional(manifest, "uri") is not { } uri || !uri.StartsWith("https://", StringComparison.Ordinal))
            {
                continue;
            }

            var query = (manifest?["queryParams"]?.AsArray() ?? [])
                .Select(p => $"{Uri.EscapeDataString(StoreHttp.Optional(p, "name") ?? string.Empty)}={Uri.EscapeDataString(StoreHttp.Optional(p, "value") ?? string.Empty)}")
                .ToList();
            manifests.Add(query.Count == 0 ? uri : $"{uri}?{string.Join('&', query)}");
        }

        return (StoreHttp.Optional(element, "buildVersion") ?? string.Empty, manifests);
    }

    public async Task<EpicInstallInfo> InstallInfoAsync(StoreSession session, string appName, string platform, CancellationToken ct)
    {
        CheckApp(appName);
        var records = await RecordsAsync(session, ct).ConfigureAwait(false);
        var (ns, item) = Find(records, appName);
        var epicPlatform = platform == "osx" ? "Mac" : "Windows";
        var (buildVersion, manifests) = await ManifestsAsync(session, epicPlatform, ns, item, appName, ct).ConfigureAwait(false)
            ?? throw new ConnectorException(ConnectorErrorKind.Unavailable, Texts.T(
                "Epic bietet für dieses Spiel keinen Download für dein System an.", "Epic offers no download of this game for your system."));
        if (manifests.Count == 0)
        {
            throw new ConnectorException(ConnectorErrorKind.Unavailable, Texts.T("Epic hat keinen Download-Link geliefert.", "Epic sent no download link."));
        }

        var catalog = await StoreHttp.GetJsonAsync(
            http,
            config.CatalogUrl.Replace("{namespace}", Uri.EscapeDataString(ns), StringComparison.Ordinal)
                .Replace("{ids}", Uri.EscapeDataString(item), StringComparison.Ordinal),
            Name, ct, Token(session)).ConfigureAwait(false);
        var details = catalog[item];
        var thirdParty = Attribute(details, "ThirdPartyManagedApp") ?? Attribute(details, "partnerLinkType");
        var launcherOnly = string.IsNullOrWhiteSpace(thirdParty)
            ? null
            : Texts.T(
                $"Dieses Spiel muss über den Epic Launcher gestartet werden (es braucht zusätzlich {thirdParty}).",
                $"This game must be started through the Epic Launcher (it also needs {thirdParty}).");
        var saveFolder = Attribute(details, platform == "osx" ? "CloudSaveFolder_MAC" : "CloudSaveFolder");
        var dlcs = launcherOnly is null
            ? await DlcsAsync(session, records, details, epicPlatform, ns, appName, ct).ConfigureAwait(false)
            : [];
        return new EpicInstallInfo(
            appName, ns, item,
            buildVersion,
            manifests,
            launcherOnly,
            string.IsNullOrWhiteSpace(saveFolder) ? null : saveFolder,
            string.Equals(Attribute(details, "OwnershipToken"), "true", StringComparison.OrdinalIgnoreCase),
            dlcs);
    }

    /// <summary>
    /// Every DLC of the game the user owns (library records in the game's namespace
    /// whose catalogue entry has a main game), marked when it has no build of its own
    /// and is only unlocked inside the game.
    /// </summary>
    public async Task<OwnedDlcs> OwnedDlcsAsync(StoreSession session, string appName, string platform, CancellationToken ct)
    {
        CheckApp(appName);
        var records = await RecordsAsync(session, ct).ConfigureAwait(false);
        var (ns, item) = Find(records, appName);
        var candidates = records
            .Where(r => r.Namespace == ns && r.CatalogItemId != item && r.AppName != appName)
            .DistinctBy(r => r.CatalogItemId)
            .ToList();
        var titles = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var chunk in candidates.Chunk(50))
        {
            // Several ids go in as repeated "id=" parameters: a comma-separated list returns nothing.
            var catalog = await StoreHttp.GetJsonAsync(
                http,
                config.CatalogUrl.Replace("{namespace}", Uri.EscapeDataString(ns), StringComparison.Ordinal)
                    .Replace("{ids}", string.Join("&id=", chunk.Select(r => Uri.EscapeDataString(r.CatalogItemId))), StringComparison.Ordinal),
                Name, ct, Token(session)).ConfigureAwait(false);
            foreach (var (id, entry) in catalog.AsObject())
            {
                if (entry?["mainGameItem"] is not null)
                {
                    titles[id] = StoreHttp.Optional(entry, "title") ?? id;
                }
            }
        }

        var epicPlatform = platform == "osx" ? "Mac" : "Windows";
        var dlcs = candidates.Where(r => titles.ContainsKey(r.CatalogItemId)).ToList();
        var result = new OwnedDlc[dlcs.Count];
        var parallel = new ParallelOptions { MaxDegreeOfParallelism = 8, CancellationToken = ct };
        await Parallel.ForEachAsync(Enumerable.Range(0, dlcs.Count), parallel, async (i, token) =>
        {
            var (_, dlcItem, dlcApp) = dlcs[i];
            var build = IsAppName(dlcApp)
                ? await ManifestsAsync(session, epicPlatform, ns, dlcItem, dlcApp, token).ConfigureAwait(false)
                : null;
            result[i] = new OwnedDlc(dlcApp.Length > 0 ? dlcApp : dlcItem, titles[dlcItem], build is not { ManifestUrls.Count: > 0 });
        }).ConfigureAwait(false);
        return new OwnedDlcs([.. result.OrderBy(d => d.Title, StringComparer.CurrentCultureIgnoreCase)]);
    }

    /// <summary>
    /// Owned DLCs of the game that come with files on this platform. DLCs that
    /// only unlock content (no build of their own) are left out: the game checks
    /// them itself through the login Cove starts it with.
    /// </summary>
    private async Task<List<EpicDlc>> DlcsAsync(
        StoreSession session,
        List<(string Namespace, string CatalogItemId, string AppName)> records,
        JsonNode? details,
        string epicPlatform,
        string ns,
        string appName,
        CancellationToken ct)
    {
        var owned = records
            .Where(r => r.Namespace == ns && r.AppName.Length > 0 && r.AppName != appName)
            .GroupBy(r => r.CatalogItemId, StringComparer.Ordinal)
            .ToDictionary(g => g.Key, g => g.First().AppName, StringComparer.Ordinal);
        var dlcs = new List<EpicDlc>();
        foreach (var dlc in details?["dlcItemList"]?.AsArray() ?? [])
        {
            if (StoreHttp.Optional(dlc, "id") is not { } id || !owned.TryGetValue(id, out var dlcApp) || !IsAppName(dlcApp))
            {
                continue;
            }

            var platforms = (dlc?["releaseInfo"]?.AsArray() ?? [])
                .Where(r => StoreHttp.Optional(r, "appId") == dlcApp)
                .SelectMany(r => r?["platform"]?.AsArray() ?? [])
                .Select(p => p is JsonValue v && v.TryGetValue(out string? name) ? name : null)
                .ToList();
            if (platforms.Count > 0 && !platforms.Contains(epicPlatform, StringComparer.OrdinalIgnoreCase))
            {
                continue;
            }

            if (await ManifestsAsync(session, epicPlatform, ns, id, dlcApp, ct).ConfigureAwait(false) is { ManifestUrls.Count: > 0 } build)
            {
                dlcs.Add(new EpicDlc(dlcApp, id, StoreHttp.Optional(dlc, "title") ?? dlcApp, build.BuildVersion, build.ManifestUrls));
            }
        }

        return dlcs;
    }

    public async Task<EpicLaunch> LaunchAsync(StoreSession session, string appName, bool ownershipToken, CancellationToken ct)
    {
        CheckApp(appName);
        var exchange = await StoreHttp.GetJsonAsync(http, config.ExchangeUrl, Name, ct, Token(session)).ConfigureAwait(false);
        string? token = null;
        if (ownershipToken)
        {
            var (ns, item) = Find(await RecordsAsync(session, ct).ConfigureAwait(false), appName);
            using var request = new HttpRequestMessage(HttpMethod.Post, Fill(config.OwnershipTokenUrl, ("accountId", session.AccountId)))
            {
                Content = new FormUrlEncodedContent(new Dictionary<string, string> { ["nsCatalogItemId"] = $"{ns}:{item}" }),
            };
            request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", Token(session));
            token = Convert.ToBase64String(await StoreHttp.SendBytesAsync(http, request, Name, ct).ConfigureAwait(false));
        }

        return new EpicLaunch(StoreHttp.Required(exchange, "code", Name), session.AccountId, session.DisplayName, token);
    }

    private string SavesUrl(StoreSession session, string appName) =>
        Fill(config.SavesUrl, ("accountId", session.AccountId), ("appName", CheckApp(appName)));

    public async Task<EpicSaves> SavesAsync(StoreSession session, string appName, CancellationToken ct)
    {
        var body = await StoreHttp.GetJsonAsync(http, SavesUrl(session, appName), Name, ct, Token(session)).ConfigureAwait(false);
        var files = new List<EpicSaveFile>();
        if (body["files"] is JsonObject list)
        {
            foreach (var (path, info) in list)
            {
                var modified = StoreHttp.Optional(info, "lastModified") is { } text && DateTimeOffset.TryParse(text, System.Globalization.CultureInfo.InvariantCulture, System.Globalization.DateTimeStyles.AssumeUniversal, out var at)
                    ? at
                    : (DateTimeOffset?)null;
                files.Add(new EpicSaveFile(path, StoreHttp.Optional(info, "readLink"), modified, StoreHttp.OptionalLong(info, "size")));
            }
        }

        return new EpicSaves(files);
    }

    /// <summary>Signed upload links for new save files (manifest and chunks).</summary>
    public async Task<EpicWriteLinks> WriteLinksAsync(StoreSession session, string appName, IReadOnlyList<string> paths, CancellationToken ct)
    {
        var prefix = $"{session.AccountId}/{CheckApp(appName)}/";
        if (paths.Count == 0 || paths.Any(p => !p.StartsWith(prefix, StringComparison.Ordinal) || p.Contains("..", StringComparison.Ordinal)))
        {
            throw new ConnectorException(ConnectorErrorKind.InvalidRequest, "invalid save paths");
        }

        var payload = new JsonObject { ["files"] = new JsonArray([.. paths.Select(p => JsonValue.Create(p))]) };
        using var request = new HttpRequestMessage(HttpMethod.Post, SavesUrl(session, appName))
        {
            Content = new StringContent(payload.ToJsonString(), System.Text.Encoding.UTF8, "application/json"),
        };
        request.Headers.Authorization = new AuthenticationHeaderValue("Bearer", Token(session));
        var body = await StoreHttp.SendJsonAsync(http, request, Name, ct).ConfigureAwait(false);
        var links = new Dictionary<string, string>(StringComparer.Ordinal);
        if (body["files"] is JsonObject files)
        {
            foreach (var (path, info) in files)
            {
                if (StoreHttp.Optional(info, "writeLink") is { } link)
                {
                    links[path] = link;
                }
            }
        }

        return links.Count == paths.Count
            ? new EpicWriteLinks(links)
            : throw new ConnectorException(ConnectorErrorKind.Unavailable, Texts.T("Epic hat keine Upload-Links für die Spielstände geliefert.", "Epic sent no upload links for the saves."));
    }
}
