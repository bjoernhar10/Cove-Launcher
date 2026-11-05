using System.Text.Json.Nodes;
using System.Text.RegularExpressions;
using System.Web;
using Cove.Connectors.Config;
using Cove.Connectors.Http;
using Cove.Connectors.Local;
using Cove.Connectors.Model;

namespace Cove.Connectors.Connectors;

/// <summary>
/// Steam: "Sign in through Steam" (OpenID 2.0) in the system browser. The
/// assertion is verified with Steam (check_authentication). Library and
/// achievements come from the Cove catalogue proxy, which holds the Steam Web
/// API key; Cove itself never needs a Steam token.
/// </summary>
public sealed partial class SteamConnector(HttpClient http, SteamConfig config) : IStoreConnector
{
    private const string OpenIdNs = "http://specs.openid.net/auth/2.0";
    private const string IdentifierSelect = "http://specs.openid.net/auth/2.0/identifier_select";

    public string Id => "steam";

    public string Name => "Steam";

    [GeneratedRegex(@"^https://steamcommunity\.com/openid/id/(\d{17})$")]
    private static partial Regex ClaimedId();

    public LoginStart LoginStart(string redirectUri, string state)
    {
        var returnTo = $"{redirectUri}?state={Uri.EscapeDataString(state)}";
        var callback = new Uri(redirectUri);
        var realm = $"{callback.Scheme}://{callback.Authority}";
        var query = string.Join('&', new Dictionary<string, string>
        {
            ["openid.ns"] = OpenIdNs,
            ["openid.mode"] = "checkid_setup",
            ["openid.return_to"] = returnTo,
            ["openid.realm"] = realm,
            ["openid.identity"] = IdentifierSelect,
            ["openid.claimed_id"] = IdentifierSelect,
        }.Select(p => $"{p.Key}={Uri.EscapeDataString(p.Value)}"));
        return new LoginStart(Id, $"{config.OpenIdEndpoint}?{query}", LoginMode.Browser, null, null);
    }

    public async Task<StoreSession> LoginCompleteAsync(LoginCompleteRequest request, StoreContext context, CancellationToken ct)
    {
        if (request.CallbackUrl is null || !Uri.TryCreate(request.CallbackUrl, UriKind.Absolute, out var callback))
        {
            throw new ConnectorException(ConnectorErrorKind.InvalidRequest, "Steam-Antwort fehlt.");
        }

        var query = HttpUtility.ParseQueryString(callback.Query);
        if (query["openid.mode"] != "id_res")
        {
            throw new ConnectorException(ConnectorErrorKind.AuthFailed, "Die Steam-Anmeldung wurde abgebrochen.");
        }

        // The assertion must be for our own loopback URL.
        if (request.ExpectedReturnTo is not null && query["openid.return_to"] != request.ExpectedReturnTo)
        {
            throw new ConnectorException(ConnectorErrorKind.AuthFailed, Texts.T("Die Steam-Antwort gehört nicht zu dieser Anmeldung.", "The Steam response doesn't belong to this sign-in."));
        }

        var match = ClaimedId().Match(query["openid.claimed_id"] ?? string.Empty);
        if (!match.Success)
        {
            throw new ConnectorException(ConnectorErrorKind.AuthFailed, Texts.T("Steam hat keine gültige Steam-ID geliefert.", "Steam didn't return a valid Steam ID."));
        }

        // Ask Steam whether it really issued this assertion.
        var fields = query.AllKeys
            .Where(k => k is not null && k.StartsWith("openid.", StringComparison.Ordinal))
            .ToDictionary(k => k!, k => k == "openid.mode" ? "check_authentication" : query[k] ?? string.Empty);
        using var verify = new HttpRequestMessage(HttpMethod.Post, config.OpenIdEndpoint) { Content = new FormUrlEncodedContent(fields) };
        var answer = await StoreHttp.SendTextAsync(http, verify, Name, ct).ConfigureAwait(false);
        if (!answer.Split('\n').Any(line => line.Trim() == "is_valid:true"))
        {
            throw new ConnectorException(ConnectorErrorKind.AuthFailed, Texts.T("Steam hat die Anmeldung nicht bestätigt.", "Steam didn't confirm the sign-in."));
        }

        var steamId = match.Groups[1].Value;
        var displayName = steamId;
        if (context.ProxyUrl is not null)
        {
            try
            {
                var profile = await StoreHttp.GetJsonAsync(http, $"{context.ProxyUrl}/v1/steam/profile/{steamId}", Name, ct).ConfigureAwait(false);
                displayName = StoreHttp.Optional(profile, "personaName") ?? steamId;
            }
            catch (ConnectorException)
            {
                // The name is cosmetic; the login itself succeeded.
            }
        }

        return new StoreSession(Id, steamId, displayName, null, null, null);
    }

    /// <summary>Steam sessions have no tokens; nothing to refresh.</summary>
    public Task<StoreSession> RefreshAsync(StoreSession session, CancellationToken ct) => Task.FromResult(session);

    private static string Proxy(StoreContext context) =>
        context.ProxyUrl ?? throw new ConnectorException(ConnectorErrorKind.Unavailable, Texts.T("Für Steam-Daten wird der Cove-Katalogdienst benötigt.", "Steam data needs the Cove catalogue service."));

    private void ThrowIfPrivate(JsonNode node)
    {
        if (node["private"]?.GetValue<bool>() == true)
        {
            throw new ConnectorException(
                ConnectorErrorKind.PrivateProfile,
                Texts.T(
                    "Dein Steam-Profil oder deine Spieldetails sind privat. Stelle „Spieldetails“ in den Steam-Privatsphäre-Einstellungen auf „Öffentlich“.",
                    "Your Steam profile or game details are private. Set “Game details” to “Public” in your Steam privacy settings."))
            {
                HelpUrl = config.PrivacySettingsUrl,
            };
        }
    }

    public async Task<IReadOnlyList<OwnedGame>> GetLibraryAsync(StoreSession session, StoreContext context, CancellationToken ct)
    {
        var body = await StoreHttp.GetJsonAsync(http, $"{Proxy(context)}/v1/steam/owned/{session.AccountId}", Name, ct).ConfigureAwait(false);
        ThrowIfPrivate(body);
        var games = new List<OwnedGame>();
        foreach (var game in body["games"]?.AsArray() ?? [])
        {
            var lastPlayed = StoreHttp.Optional(game, "lastPlayedAt");
            games.Add(new OwnedGame(
                StoreHttp.Required(game, "appId", Name),
                StoreHttp.Optional(game, "name") ?? "Unbekanntes Spiel",
                StoreHttp.OptionalLong(game, "playtimeMinutes"),
                lastPlayed is null ? null : DateTimeOffset.Parse(lastPlayed, System.Globalization.CultureInfo.InvariantCulture),
                StoreHttp.Optional(game, "iconUrl")));
        }

        return games;
    }

    public async Task<IReadOnlyList<StoreAchievement>> GetAchievementsAsync(
        StoreSession session, string storeGameId, StoreContext context, CancellationToken ct)
    {
        if (!storeGameId.All(char.IsAsciiDigit))
        {
            throw new ConnectorException(ConnectorErrorKind.InvalidRequest, Texts.T($"Ungültige Steam-App-ID: {storeGameId}", $"Invalid Steam app ID: {storeGameId}"));
        }

        var body = await StoreHttp.GetJsonAsync(
            http, $"{Proxy(context)}/v1/steam/achievements/{session.AccountId}/{storeGameId}", Name, ct).ConfigureAwait(false);
        ThrowIfPrivate(body);
        var result = new List<StoreAchievement>();
        foreach (var a in body["achievements"]?.AsArray() ?? [])
        {
            var unlocked = StoreHttp.Optional(a, "unlockedAt");
            result.Add(new StoreAchievement(
                StoreHttp.Required(a, "id", Name),
                StoreHttp.Optional(a, "title") ?? StoreHttp.Required(a, "id", Name),
                StoreHttp.Optional(a, "description") ?? string.Empty,
                StoreHttp.Optional(a, "iconUrl"),
                StoreHttp.Optional(a, "iconLockedUrl"),
                a?["hidden"]?.GetValue<bool>() == true,
                unlocked is null ? null : DateTimeOffset.Parse(unlocked, System.Globalization.CultureInfo.InvariantCulture),
                a?["rarityPercent"] is JsonValue r && r.TryGetValue(out double rarity) ? rarity : null));
        }

        return result;
    }

    public IReadOnlyList<LocalGame> DetectLocal(LocalEnvironment environment) => LocalDetection.DetectSteam(environment);
}
