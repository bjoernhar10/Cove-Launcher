using System.Net.Http.Headers;
using System.Text;
using System.Text.Json.Nodes;
using Cove.Connectors.Config;
using Cove.Connectors.Http;
using Cove.Connectors.Local;
using Cove.Connectors.Model;

namespace Cove.Connectors.Connectors;

/// <summary>
/// Epic: the official login page in a webview with the launcher client (as
/// Legendary/Heroic). The redirect page returns the authorization code as JSON;
/// the core reads it (or the user pastes it in the alternative login).
/// </summary>
public sealed class EpicConnector(HttpClient http, EpicConfig config) : IStoreConnector
{
    /// <summary>First pause before retrying a catalogue request (doubles each time); zero in tests.</summary>
    public TimeSpan RetryDelay { get; init; } = TimeSpan.FromSeconds(1);

    /// <summary>Catalogue requests in flight at once; Epic answered 8 without rate limits.</summary>
    private const int CatalogParallelism = 8;

    /// <summary>Cover art by preference: portrait box art first, landscape last (the card crops it).</summary>
    private static readonly string[] CoverImageTypes =
        ["DieselGameBoxTall", "OfferImageTall", "DieselStoreFrontTall", "DieselGameBox", "OfferImageWide", "DieselStoreFrontWide", "Thumbnail"];

    public string Id => "epic";

    public string Name => "Epic";

    private string RedirectUrl => config.RedirectUrl.Replace("{clientId}", config.ClientId, StringComparison.Ordinal);

    public LoginStart LoginStart(string redirectUri, string state)
    {
        var url = config.LoginUrl.Replace("{redirect}", Uri.EscapeDataString(RedirectUrl), StringComparison.Ordinal);
        return new LoginStart(Id, url, LoginMode.Webview, RedirectUrl.Split('?')[0], url);
    }

    private async Task<StoreSession> TokenAsync(Dictionary<string, string> form, CancellationToken ct)
    {
        using var request = new HttpRequestMessage(HttpMethod.Post, config.TokenUrl) { Content = new FormUrlEncodedContent(form) };
        request.Headers.Authorization = new AuthenticationHeaderValue(
            "Basic", Convert.ToBase64String(Encoding.ASCII.GetBytes($"{config.ClientId}:{config.ClientSecret}")));
        var body = await StoreHttp.SendJsonAsync(http, request, Name, ct).ConfigureAwait(false);
        var expires = StoreHttp.Optional(body, "expires_at");
        return new StoreSession(
            Id,
            StoreHttp.Required(body, "account_id", Name),
            StoreHttp.Optional(body, "displayName") ?? StoreHttp.Required(body, "account_id", Name),
            StoreHttp.Required(body, "access_token", Name),
            StoreHttp.Required(body, "refresh_token", Name),
            expires is null
                ? DateTimeOffset.UtcNow.AddSeconds(StoreHttp.OptionalLong(body, "expires_in") ?? 7200)
                : DateTimeOffset.Parse(expires, System.Globalization.CultureInfo.InvariantCulture));
    }

    /// <summary>Accepts the bare code or the whole JSON the redirect page shows.</summary>
    public static string? ExtractCode(string? input)
    {
        var text = input?.Trim();
        if (string.IsNullOrEmpty(text))
        {
            return null;
        }

        if (text.StartsWith('{'))
        {
            try
            {
                return StoreHttp.Optional(JsonNode.Parse(text), "authorizationCode");
            }
            catch (System.Text.Json.JsonException)
            {
                return null;
            }
        }

        return text.All(char.IsAsciiLetterOrDigit) ? text : null;
    }

    public Task<StoreSession> LoginCompleteAsync(LoginCompleteRequest request, StoreContext context, CancellationToken ct)
    {
        var code = ExtractCode(request.Code)
            ?? throw new ConnectorException(ConnectorErrorKind.InvalidRequest, Texts.T("Das ist kein gültiger Epic-Anmeldecode.", "That's not a valid Epic sign-in code."));
        return TokenAsync(new() { ["grant_type"] = "authorization_code", ["code"] = code, ["token_type"] = "eg1" }, ct);
    }

    public Task<StoreSession> RefreshAsync(StoreSession session, CancellationToken ct)
    {
        if (session.RefreshToken is null)
        {
            throw new ConnectorException(ConnectorErrorKind.AuthFailed, "Bitte melde dich erneut bei Epic an.");
        }

        return TokenAsync(new() { ["grant_type"] = "refresh_token", ["refresh_token"] = session.RefreshToken, ["token_type"] = "eg1" }, ct);
    }

    public async Task<IReadOnlyList<OwnedGame>> GetLibraryAsync(StoreSession session, StoreContext context, CancellationToken ct)
    {
        // 1. Library records (paged).
        var records = new List<(string Namespace, string CatalogItemId, string AppName)>();
        string? cursor = null;
        for (var i = 0; i < 100; i++)
        {
            var url = cursor is null ? config.LibraryUrl : $"{config.LibraryUrl}&cursor={Uri.EscapeDataString(cursor)}";
            var page = await StoreHttp.GetJsonAsync(http, url, Name, ct, session.AccessToken).ConfigureAwait(false);
            foreach (var record in page["records"]?.AsArray() ?? [])
            {
                var ns = StoreHttp.Optional(record, "namespace");
                var item = StoreHttp.Optional(record, "catalogItemId");
                var app = StoreHttp.Optional(record, "appName");
                // "ue" = Unreal Engine marketplace assets, not games.
                if (ns is not null && item is not null && app is not null && ns != "ue")
                {
                    records.Add((ns, item, app));
                }
            }

            cursor = StoreHttp.Optional(page["responseMetadata"], "nextCursor");
            if (string.IsNullOrEmpty(cursor))
            {
                break;
            }
        }

        // 2. Titles and box art from the catalogue; DLCs (items with a main game) and
        // Unreal Engine / Fab assets (category "asset-format…") are skipped.
        // Several ids go in as repeated "id=" parameters: a comma-separated list returns nothing.
        // One request per namespace, so a large library sends hundreds: they run a few at a
        // time (one after another took minutes and ran into the core's timeout), and rate
        // limits and hiccups are retried, otherwise the game would only get its cryptic app name.
        var titles = new Dictionary<string, string>(StringComparer.Ordinal);
        var images = new Dictionary<string, string>(StringComparer.Ordinal);
        var skipped = new HashSet<string>(StringComparer.Ordinal);
        var requests = records
            .GroupBy(r => r.Namespace)
            .SelectMany(group => group.Select(r => r.CatalogItemId).Distinct(StringComparer.Ordinal).Chunk(50)
                .Select(ids => config.CatalogUrl
                    .Replace("{namespace}", Uri.EscapeDataString(group.Key), StringComparison.Ordinal)
                    .Replace("{ids}", string.Join("&id=", ids.Select(Uri.EscapeDataString)), StringComparison.Ordinal)))
            .ToList();
        var parallel = new ParallelOptions { MaxDegreeOfParallelism = CatalogParallelism, CancellationToken = ct };
        await Parallel.ForEachAsync(requests, parallel, async (url, token) =>
        {
            if (await CatalogAsync(url, session, token).ConfigureAwait(false) is not JsonObject items)
            {
                return;
            }

            lock (titles)
            {
                foreach (var (id, item) in items)
                {
                    if (item is not JsonObject)
                    {
                        continue;
                    }

                    if (item["mainGameItem"] is not null || IsAsset(item))
                    {
                        skipped.Add(id);
                    }
                    else if (StoreHttp.Optional(item, "title") is { } title)
                    {
                        titles[id] = title;
                        if (CoverImage(item) is { } image)
                        {
                            images[id] = image;
                        }
                    }
                }
            }
        }).ConfigureAwait(false);

        // 3. Playtime (optional).
        var playtime = new Dictionary<string, long>(StringComparer.Ordinal);
        try
        {
            var url = config.PlaytimeUrl.Replace("{accountId}", Uri.EscapeDataString(session.AccountId), StringComparison.Ordinal);
            var entries = await StoreHttp.GetJsonAsync(http, url, Name, ct, session.AccessToken).ConfigureAwait(false);
            foreach (var entry in entries.AsArray())
            {
                if (StoreHttp.Optional(entry, "artifactId") is { } artifact && StoreHttp.OptionalLong(entry, "totalTime") is { } seconds)
                {
                    playtime[artifact] = seconds / 60;
                }
            }
        }
        catch (Exception e) when (e is ConnectorException or InvalidOperationException)
        {
            // Playtime is a bonus; the library is still useful without it.
        }

        return [.. records
            .Where(r => !skipped.Contains(r.CatalogItemId))
            .DistinctBy(r => r.AppName)
            .Select(r => new OwnedGame(
                r.AppName,
                titles.GetValueOrDefault(r.CatalogItemId) ?? r.AppName,
                playtime.TryGetValue(r.AppName, out var minutes) ? minutes : null,
                null,
                images.GetValueOrDefault(r.CatalogItemId)))];
    }

    /// <summary>
    /// Fab / Unreal Engine marketplace content (models, sample projects): Epic files it under
    /// "asset-format…" categories, which no game carries. The old marketplace namespace "ue" is
    /// skipped earlier; Fab content lives in namespaces of its own.
    /// </summary>
    private static bool IsAsset(JsonNode item) =>
        (item["categories"]?.AsArray() ?? []).Any(c =>
            StoreHttp.Optional(c, "path") is { } path &&
            (path == "asset-format" || path.StartsWith("asset-format/", StringComparison.Ordinal)));

    /// <summary>Catalogue items, or null when Epic still fails after a few tries.</summary>
    private async Task<JsonNode?> CatalogAsync(string url, StoreSession session, CancellationToken ct)
    {
        var delay = RetryDelay;
        for (var attempt = 1; ; attempt++)
        {
            try
            {
                return await StoreHttp.GetJsonAsync(http, url, Name, ct, session.AccessToken).ConfigureAwait(false);
            }
            catch (ConnectorException e) when (e.Kind is ConnectorErrorKind.Unavailable or ConnectorErrorKind.Network)
            {
                if (attempt == 4)
                {
                    return null; // the title falls back to the app name
                }
            }

            await Task.Delay(delay, ct).ConfigureAwait(false);
            delay *= 2;
        }
    }

    /// <summary>Best box art of a catalogue item, scaled down by Epic's image CDN.</summary>
    private static string? CoverImage(JsonNode item)
    {
        var byType = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var image in item["keyImages"]?.AsArray() ?? [])
        {
            if (StoreHttp.Optional(image, "type") is { } type && StoreHttp.Optional(image, "url") is { } url &&
                url.StartsWith("https://", StringComparison.Ordinal))
            {
                byType.TryAdd(type, url);
            }
        }

        foreach (var type in CoverImageTypes)
        {
            if (byType.TryGetValue(type, out var url))
            {
                // Box art is often several megabytes; cdn1.epicgames.com resizes on request.
                return url.Contains('?', StringComparison.Ordinal) || !url.StartsWith("https://cdn1.epicgames.com/", StringComparison.Ordinal)
                    ? url
                    : $"{url}?h=900&resize=1&w=600";
            }
        }

        return null;
    }

    /// <summary>Epic achievements use a separate, undocumented GraphQL API; not supported yet.</summary>
    public Task<IReadOnlyList<StoreAchievement>> GetAchievementsAsync(
        StoreSession session, string storeGameId, StoreContext context, CancellationToken ct) =>
        Task.FromResult<IReadOnlyList<StoreAchievement>>([]);

    public IReadOnlyList<LocalGame> DetectLocal(LocalEnvironment environment) =>
        [.. LocalDetection.DetectAll(environment).Where(g => g.Store == Id)];
}
