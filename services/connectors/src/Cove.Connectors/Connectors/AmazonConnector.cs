using System.Security.Cryptography;
using System.Text;
using System.Text.Json.Nodes;
using Cove.Connectors.Config;
using Cove.Connectors.Http;
using Cove.Connectors.Model;

namespace Cove.Connectors.Connectors;

/// <summary>
/// Amazon Games (Prime Gaming): Amazon's sign-in page in a webview, with the
/// device registration of the Amazon Games app (PKCE). The core reads the code
/// from the landing page and hands back the login's <c>state</c>, from which the
/// PKCE verifier and the device serial are derived, so nothing is kept here
/// between calls. The session's <see cref="StoreSession.AccountId"/> is that
/// device serial: Amazon's library API asks for its hash.
/// </summary>
public sealed class AmazonConnector(HttpClient http, AmazonConfig config) : IStoreConnector
{
    public string Id => "amazon";

    public string Name => "Amazon";

    /// <summary>PKCE verifier for a login (43 URL-safe characters).</summary>
    public static string Verifier(string state) =>
        Base64Url(SHA256.HashData(Encoding.UTF8.GetBytes("cove-amazon-verifier:" + state)));

    /// <summary>Device serial for a login (32 upper-case hex characters).</summary>
    public static string Serial(string state) =>
        Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes("cove-amazon-serial:" + state)))[..32];

    public string ClientId(string serial) =>
        Convert.ToHexStringLower(Encoding.ASCII.GetBytes($"{serial}#{config.DeviceType}"));

    private static string Base64Url(byte[] bytes) =>
        Convert.ToBase64String(bytes).TrimEnd('=').Replace('+', '-').Replace('/', '_');

    public LoginStart LoginStart(string redirectUri, string state)
    {
        var challenge = Base64Url(SHA256.HashData(Encoding.ASCII.GetBytes(Verifier(state))));
        var query = new (string Key, string Value)[]
        {
            ("openid.ns", "http://specs.openid.net/auth/2.0"),
            ("openid.claimed_id", "http://specs.openid.net/auth/2.0/identifier_select"),
            ("openid.identity", "http://specs.openid.net/auth/2.0/identifier_select"),
            ("openid.mode", "checkid_setup"),
            ("openid.oa2.scope", "device_auth_access"),
            ("openid.ns.oa2", "http://www.amazon.com/ap/ext/oauth/2"),
            ("openid.oa2.response_type", "code"),
            ("openid.oa2.code_challenge_method", "S256"),
            ("openid.oa2.client_id", "device:" + ClientId(Serial(state))),
            ("language", "en_US"),
            ("marketPlaceId", config.MarketplaceId),
            ("openid.return_to", config.ReturnTo),
            ("openid.pape.max_auth_age", "0"),
            ("openid.assoc_handle", "amzn_sonic_games_launcher"),
            ("pageId", "amzn_sonic_games_launcher"),
            ("openid.oa2.code_challenge", challenge),
        };
        var url = config.SigninUrl + "?" + string.Join('&', query.Select(q => $"{Uri.EscapeDataString(q.Key)}={Uri.EscapeDataString(q.Value)}"));
        return new LoginStart(Id, url, LoginMode.Webview, config.ReturnTo + "/?openid", null);
    }

    private HttpRequestMessage Post(string url, JsonObject body)
    {
        var request = new HttpRequestMessage(HttpMethod.Post, url)
        {
            Content = new StringContent(body.ToJsonString(), Encoding.UTF8, "application/json"),
        };
        request.Headers.UserAgent.Clear();
        request.Headers.TryAddWithoutValidation("User-Agent", config.UserAgent);
        return request;
    }

    public async Task<StoreSession> LoginCompleteAsync(LoginCompleteRequest request, StoreContext context, CancellationToken ct)
    {
        var state = request.ExpectedReturnTo;
        if (string.IsNullOrWhiteSpace(request.Code) || string.IsNullOrEmpty(state))
        {
            throw new ConnectorException(ConnectorErrorKind.InvalidRequest, Texts.T("Amazon hat keinen Anmeldecode geliefert.", "Amazon didn't return a sign-in code."));
        }

        var serial = Serial(state);
        var body = new JsonObject
        {
            ["auth_data"] = new JsonObject
            {
                ["authorization_code"] = request.Code.Trim(),
                ["client_domain"] = "DeviceLegacy",
                ["client_id"] = ClientId(serial),
                ["code_algorithm"] = "SHA-256",
                ["code_verifier"] = Verifier(state),
                ["use_global_authentication"] = false,
            },
            ["registration_data"] = new JsonObject
            {
                ["app_name"] = config.AppName,
                ["app_version"] = config.AppVersion,
                ["device_model"] = "Windows",
                ["device_name"] = null,
                ["device_serial"] = serial,
                ["device_type"] = config.DeviceType,
                ["domain"] = "Device",
                ["os_version"] = "10.0.19044.0",
            },
            ["requested_extensions"] = new JsonArray("customer_info", "device_info"),
            ["requested_token_type"] = new JsonArray("bearer", "mac_dms"),
            ["user_context_map"] = new JsonObject(),
        };
        using var message = Post(config.RegisterUrl, body);
        var answer = await StoreHttp.SendJsonAsync(http, message, Name, ct).ConfigureAwait(false);
        var success = answer["response"]?["success"]
            ?? throw new ConnectorException(ConnectorErrorKind.AuthFailed, Texts.T("Amazon hat die Anmeldung abgelehnt.", "Amazon declined the sign-in."));
        var bearer = success["tokens"]?["bearer"];
        var customer = success["extensions"]?["customer_info"];
        var deviceSerial = StoreHttp.Optional(success["extensions"]?["device_info"], "device_serial_number") ?? serial;
        return new StoreSession(
            Id,
            deviceSerial,
            StoreHttp.Optional(customer, "given_name") ?? StoreHttp.Optional(customer, "name") ?? "Amazon",
            StoreHttp.Required(bearer, "access_token", Name),
            StoreHttp.Required(bearer, "refresh_token", Name),
            DateTimeOffset.UtcNow.AddSeconds(ExpiresIn(bearer)));
    }

    /// <summary>Amazon sends <c>expires_in</c> as a number or a string.</summary>
    private static long ExpiresIn(JsonNode? node) =>
        long.TryParse(StoreHttp.Optional(node, "expires_in"), out var seconds) && seconds > 0 ? seconds : 3600;

    public async Task<StoreSession> RefreshAsync(StoreSession session, CancellationToken ct)
    {
        if (session.RefreshToken is null)
        {
            throw new ConnectorException(ConnectorErrorKind.AuthFailed, Texts.T("Bitte melde dich erneut bei Amazon an.", "Please sign in to Amazon again."));
        }

        var body = new JsonObject
        {
            ["source_token"] = session.RefreshToken,
            ["source_token_type"] = "refresh_token",
            ["requested_token_type"] = "access_token",
            ["app_name"] = config.AppName,
            ["app_version"] = config.AppVersion,
        };
        using var message = Post(config.TokenUrl, body);
        var answer = await StoreHttp.SendJsonAsync(http, message, Name, ct).ConfigureAwait(false);
        return session with
        {
            AccessToken = StoreHttp.Required(answer, "access_token", Name),
            ExpiresAt = DateTimeOffset.UtcNow.AddSeconds(ExpiresIn(answer)),
        };
    }

    /// <summary>A request to Amazon's game distribution service.</summary>
    internal HttpRequestMessage Distribution(string url, string target, string accessToken, JsonObject body)
    {
        var request = Post(url, body);
        request.Headers.TryAddWithoutValidation("X-Amz-Target", target);
        request.Headers.TryAddWithoutValidation("x-amzn-token", accessToken);
        request.Headers.TryAddWithoutValidation("UserAgent", config.LauncherAgent);
        request.Content!.Headers.ContentEncoding.Add("amz-1.0");
        return request;
    }

    /// <summary>Every entitlement of the account (games and their product data).</summary>
    public async Task<IReadOnlyList<JsonNode>> EntitlementsAsync(StoreSession session, CancellationToken ct)
    {
        var all = new List<JsonNode>();
        var seen = new HashSet<string>(StringComparer.Ordinal);
        string? next = null;
        for (var page = 0; page < 200; page++)
        {
            var body = new JsonObject
            {
                ["Operation"] = "GetEntitlements",
                ["clientId"] = "Sonic",
                ["syncPoint"] = null,
                ["nextToken"] = next,
                ["maxResults"] = 50,
                ["productIdFilter"] = null,
                ["keyId"] = config.KeyId,
                ["hardwareHash"] = Convert.ToHexString(SHA256.HashData(Encoding.ASCII.GetBytes(session.AccountId))),
            };
            using var request = Distribution(
                config.EntitlementsUrl,
                "com.amazon.animusdistributionservice.entitlement.AnimusEntitlementsService.GetEntitlements",
                session.AccessToken ?? "",
                body);
            var answer = await StoreHttp.SendJsonAsync(http, request, Name, ct).ConfigureAwait(false);
            foreach (var entitlement in answer["entitlements"]?.AsArray() ?? [])
            {
                var productId = StoreHttp.Optional(entitlement?["product"], "id");
                if (entitlement is not null && productId is not null && seen.Add(productId))
                {
                    all.Add(entitlement);
                }
            }

            next = StoreHttp.Optional(answer, "nextToken");
            if (string.IsNullOrEmpty(next))
            {
                break;
            }
        }

        return all;
    }

    public async Task<IReadOnlyList<OwnedGame>> GetLibraryAsync(StoreSession session, StoreContext context, CancellationToken ct)
    {
        var entitlements = await EntitlementsAsync(session, ct).ConfigureAwait(false);
        return [.. entitlements.Select(e =>
        {
            var product = e["product"];
            var details = product?["productDetail"]?["details"];
            var image = StoreHttp.Optional(product?["productDetail"], "iconUrl")
                ?? StoreHttp.Optional(details, "logoUrl");
            return new OwnedGame(
                StoreHttp.Required(product, "id", Name),
                StoreHttp.Optional(product, "title") ?? Texts.T("Unbekanntes Spiel", "Unknown game"),
                null,
                null,
                image);
        })];
    }

    /// <summary>Amazon has no achievements of its own.</summary>
    public Task<IReadOnlyList<StoreAchievement>> GetAchievementsAsync(
        StoreSession session, string storeGameId, StoreContext context, CancellationToken ct) =>
        Task.FromResult<IReadOnlyList<StoreAchievement>>([]);

    /// <summary>Games of the Amazon Games app are not detected yet.</summary>
    public IReadOnlyList<LocalGame> DetectLocal(LocalEnvironment environment) => [];
}
