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
            ?? throw new ConnectorException(ConnectorErrorKind.InvalidRequest, "Das ist kein gültiger Epic-Anmeldecode.");
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

        // 2. Titles from the catalogue; DLCs (items with a main game) are skipped.
        var titles = new Dictionary<string, string>(StringComparer.Ordinal);
        var dlcs = new HashSet<string>(StringComparer.Ordinal);
        foreach (var group in records.GroupBy(r => r.Namespace))
        {
            foreach (var chunk in group.Chunk(50))
            {
                var url = config.CatalogUrl
                    .Replace("{namespace}", Uri.EscapeDataString(group.Key), StringComparison.Ordinal)
                    .Replace("{ids}", string.Join(',', chunk.Select(r => r.CatalogItemId)), StringComparison.Ordinal);
                JsonNode catalog;
                try
                {
                    catalog = await StoreHttp.GetJsonAsync(http, url, Name, ct, session.AccessToken).ConfigureAwait(false);
                }
                catch (ConnectorException e) when (e.Kind == ConnectorErrorKind.Unavailable)
                {
                    continue; // titles fall back to the app name
                }

                if (catalog is not JsonObject items)
                {
                    continue;
                }

                foreach (var (id, item) in items)
                {
                    if (item is not JsonObject)
                    {
                        continue;
                    }

                    if (item["mainGameItem"] is not null)
                    {
                        dlcs.Add(id);
                    }
                    else if (StoreHttp.Optional(item, "title") is { } title)
                    {
                        titles[id] = title;
                    }
                }
            }
        }

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
            .Where(r => !dlcs.Contains(r.CatalogItemId))
            .DistinctBy(r => r.AppName)
            .Select(r => new OwnedGame(
                r.AppName,
                titles.GetValueOrDefault(r.CatalogItemId) ?? r.AppName,
                playtime.TryGetValue(r.AppName, out var minutes) ? minutes : null,
                null,
                null))];
    }

    /// <summary>Epic achievements use a separate, undocumented GraphQL API; not supported yet.</summary>
    public Task<IReadOnlyList<StoreAchievement>> GetAchievementsAsync(
        StoreSession session, string storeGameId, StoreContext context, CancellationToken ct) =>
        Task.FromResult<IReadOnlyList<StoreAchievement>>([]);

    public IReadOnlyList<LocalGame> DetectLocal(LocalEnvironment environment) =>
        [.. LocalDetection.DetectAll(environment).Where(g => g.Store == Id)];
}
