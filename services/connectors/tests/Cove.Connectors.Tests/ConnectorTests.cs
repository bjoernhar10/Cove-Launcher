using System.Net;
using System.Web;
using Cove.Connectors.Connectors;
using Cove.Connectors.Model;
using Xunit;

namespace Cove.Connectors.Tests;

public class SteamConnectorTests
{
    private const string Loopback = "http://127.0.0.1:53111/callback";
    private const string ReturnTo = Loopback + "?state=s1";
    private const string Proxy = "https://proxy.test";

    private static string Callback(string claimedId = "https://steamcommunity.com/openid/id/76561197960287930", string returnTo = ReturnTo) =>
        $"{ReturnTo}&openid.ns=http%3A%2F%2Fspecs.openid.net%2Fauth%2F2.0&openid.mode=id_res" +
        $"&openid.op_endpoint=https%3A%2F%2Fsteamcommunity.com%2Fopenid%2Flogin&openid.claimed_id={Uri.EscapeDataString(claimedId)}" +
        $"&openid.identity={Uri.EscapeDataString(claimedId)}&openid.return_to={Uri.EscapeDataString(returnTo)}" +
        "&openid.response_nonce=2026-11-04T10%3A00%3A00Zabc&openid.assoc_handle=1234567890" +
        "&openid.signed=signed%2Cop_endpoint%2Cclaimed_id%2Cidentity%2Creturn_to%2Cresponse_nonce%2Cassoc_handle&openid.sig=abc%3D";

    private static (SteamConnector Connector, StubHandler Handler) Create(string verdict = "ns:http://specs.openid.net/auth/2.0\nis_valid:true\n")
    {
        var handler = new StubHandler((request, _) => request.RequestUri!.AbsolutePath switch
        {
            "/openid/login" => StubHandler.Text(verdict),
            "/v1/steam/profile/76561197960287930" => StubHandler.Json("""{ "steamId": "76561197960287930", "personaName": "Gabe", "public": true }"""),
            "/v1/steam/owned/76561197960287930" => StubHandler.Json("""
                { "private": false, "games": [
                  { "appId": "620", "name": "Portal 2", "playtimeMinutes": 754, "lastPlayedAt": "2026-10-01T10:00:00Z", "iconUrl": null },
                  { "appId": "400", "name": "Portal", "playtimeMinutes": 0, "lastPlayedAt": null, "iconUrl": null } ] }
                """),
            "/v1/steam/owned/76561197960287931" => StubHandler.Json("""{ "private": true, "games": [] }"""),
            "/v1/steam/achievements/76561197960287930/620" => StubHandler.Json("""
                { "private": false, "achievements": [
                  { "id": "ACH_WAKE_UP", "title": "Wake Up Call", "description": "Survive", "iconUrl": "https://cdn/a.jpg",
                    "iconLockedUrl": "https://cdn/b.jpg", "hidden": false, "unlockedAt": "2026-09-01T10:00:00Z", "rarityPercent": 81.5 },
                  { "id": "ACH_SECRET", "title": "Secret", "description": "", "iconUrl": null, "iconLockedUrl": null,
                    "hidden": true, "unlockedAt": null, "rarityPercent": null } ] }
                """),
            _ => StubHandler.Text("not found", HttpStatusCode.NotFound),
        });
        return (new SteamConnector(new HttpClient(handler), TestConfig.Default.Steam), handler);
    }

    [Fact]
    public void LoginStartBuildsAnOpenIdRequestForTheLoopbackUrl()
    {
        var (steam, _) = Create();
        var start = steam.LoginStart(Loopback, "s1");
        Assert.Equal(LoginMode.Browser, start.Mode);
        var query = HttpUtility.ParseQueryString(new Uri(start.Url).Query);
        Assert.Equal("checkid_setup", query["openid.mode"]);
        Assert.Equal(ReturnTo, query["openid.return_to"]);
        Assert.Equal("http://127.0.0.1:53111", query["openid.realm"]);
        Assert.StartsWith("https://steamcommunity.com/openid/login?", start.Url, StringComparison.Ordinal);
    }

    [Fact]
    public async Task LoginCompleteVerifiesTheAssertionWithSteam()
    {
        var (steam, handler) = Create();
        var session = await steam.LoginCompleteAsync(new LoginCompleteRequest(Callback(), null, ReturnTo), new StoreContext(Proxy), TestContext.Current.CancellationToken);
        Assert.Equal(("steam", "76561197960287930", "Gabe"), (session.Store, session.AccountId, session.DisplayName));
        Assert.Null(session.AccessToken);

        var check = handler.Requests.Single(r => r.Request.RequestUri!.AbsolutePath == "/openid/login");
        Assert.Equal(HttpMethod.Post, check.Request.Method);
        Assert.Contains("openid.mode=check_authentication", check.Body, StringComparison.Ordinal);
        Assert.Contains("openid.sig=abc%3D", check.Body, StringComparison.Ordinal);
    }

    [Fact]
    public async Task RejectsForgedOrForeignAssertions()
    {
        var (forged, _) = Create("is_valid:false\n");
        var e = await Assert.ThrowsAsync<ConnectorException>(() =>
            forged.LoginCompleteAsync(new LoginCompleteRequest(Callback(), null, ReturnTo), new StoreContext(Proxy), TestContext.Current.CancellationToken));
        Assert.Equal(ConnectorErrorKind.AuthFailed, e.Kind);

        var (steam, _) = Create();
        var foreign = await Assert.ThrowsAsync<ConnectorException>(() =>
            steam.LoginCompleteAsync(new LoginCompleteRequest(Callback(returnTo: "http://evil.test/cb"), null, ReturnTo), new StoreContext(Proxy), TestContext.Current.CancellationToken));
        Assert.Equal(ConnectorErrorKind.AuthFailed, foreign.Kind);

        var noId = await Assert.ThrowsAsync<ConnectorException>(() =>
            steam.LoginCompleteAsync(new LoginCompleteRequest(Callback("https://evil.test/id/1"), null, ReturnTo), new StoreContext(Proxy), TestContext.Current.CancellationToken));
        Assert.Equal(ConnectorErrorKind.AuthFailed, noId.Kind);
    }

    [Fact]
    public async Task ReadsLibraryAndAchievementsThroughTheProxy()
    {
        var (steam, _) = Create();
        var session = new StoreSession("steam", "76561197960287930", "Gabe", null, null, null);
        var games = await steam.GetLibraryAsync(session, new StoreContext(Proxy), TestContext.Current.CancellationToken);
        Assert.Equal(["620", "400"], games.Select(g => g.StoreGameId));
        Assert.Equal(754, games[0].PlaytimeMinutes);
        Assert.Equal("https://shared.akamai.steamstatic.com/store_item_assets/steam/apps/620/library_600x900_2x.jpg", games[0].ImageUrl);

        var achievements = await steam.GetAchievementsAsync(session, "620", new StoreContext(Proxy), TestContext.Current.CancellationToken);
        Assert.Equal(81.5, achievements[0].RarityPercent);
        Assert.NotNull(achievements[0].UnlockedAt);
        Assert.True(achievements[1].Hidden);
        await Assert.ThrowsAsync<ConnectorException>(() => steam.GetAchievementsAsync(session, "../x", new StoreContext(Proxy), TestContext.Current.CancellationToken));
    }

    [Fact]
    public async Task PrivateProfilesAreReportedAsSuch()
    {
        var (steam, _) = Create();
        var e = await Assert.ThrowsAsync<ConnectorException>(() =>
            steam.GetLibraryAsync(new StoreSession("steam", "76561197960287931", "x", null, null, null), new StoreContext(Proxy), TestContext.Current.CancellationToken));
        Assert.Equal(ConnectorErrorKind.PrivateProfile, e.Kind);
        Assert.Equal("https://steamcommunity.com/my/edit/settings", e.HelpUrl);
    }
}

public class GogConnectorTests
{
    [Fact]
    public async Task ExchangesTheCodeAndRefreshes()
    {
        var handler = new StubHandler((request, _) =>
        {
            var url = request.RequestUri!;
            if (url.Host == "auth.gog.com" && url.Query.Contains("grant_type=refresh_token&refresh_token=dead", StringComparison.Ordinal))
            {
                return StubHandler.Json("""{ "error": "invalid_grant" }""", HttpStatusCode.BadRequest);
            }

            return url.AbsolutePath switch
            {
                "/token" => StubHandler.Json("""{ "access_token": "a1", "refresh_token": "r1", "expires_in": 3600, "user_id": "4815" }"""),
                "/userData.json" => StubHandler.Json("""{ "username": "kiefernzapfen" }"""),
                _ => StubHandler.Text("", HttpStatusCode.NotFound),
            };
        });
        var gog = new GogConnector(new HttpClient(handler), TestConfig.Default.Gog);

        var start = gog.LoginStart("unused", "unused");
        Assert.Equal((LoginMode.Webview, "https://embed.gog.com/on_login_success"), (start.Mode, start.CallbackPrefix));

        var session = await gog.LoginCompleteAsync(new LoginCompleteRequest(null, "c0de", null), new StoreContext(null), TestContext.Current.CancellationToken);
        Assert.Equal(("4815", "kiefernzapfen", "a1", "r1"), (session.AccountId, session.DisplayName, session.AccessToken, session.RefreshToken));
        var token = handler.Requests.First().Request.RequestUri!.Query;
        Assert.Contains("grant_type=authorization_code&code=c0de", token, StringComparison.Ordinal);
        Assert.Contains("client_id=46899977096215655", token, StringComparison.Ordinal);
        Assert.Equal("Bearer", handler.Requests[1].Request.Headers.Authorization!.Scheme);

        var refreshed = await gog.RefreshAsync(session, TestContext.Current.CancellationToken);
        Assert.Equal("kiefernzapfen", refreshed.DisplayName);
        var dead = await Assert.ThrowsAsync<ConnectorException>(() => gog.RefreshAsync(session with { RefreshToken = "dead" }, TestContext.Current.CancellationToken));
        Assert.Equal(ConnectorErrorKind.AuthFailed, dead.Kind);
    }

    [Fact]
    public async Task ReadsAllLibraryPages()
    {
        var handler = new StubHandler((request, _) => request.RequestUri!.Query.Contains("page=1", StringComparison.Ordinal)
            ? StubHandler.Json("""{ "totalPages": 2, "products": [ { "id": 1207664663, "title": "The Witcher 3", "image": "//images.gog.com/abc" } ] }""")
            : StubHandler.Json("""{ "totalPages": 2, "products": [ { "id": 1207658924, "title": "Beneath a Steel Sky" } ] }"""));
        var gog = new GogConnector(new HttpClient(handler), TestConfig.Default.Gog);
        var games = await gog.GetLibraryAsync(new StoreSession("gog", "1", "x", "a", "r", null), new StoreContext(null), TestContext.Current.CancellationToken);
        Assert.Equal(["1207664663", "1207658924"], games.Select(g => g.StoreGameId));
        Assert.Equal("https://images.gog.com/abc.jpg", games[0].ImageUrl);
        Assert.Equal(2, handler.Requests.Count);
    }
}

public class EpicConnectorTests
{
    [Theory]
    [InlineData("""{"redirectUrl":"https://localhost","authorizationCode":"abc123","sid":null}""", "abc123")]
    [InlineData("  abc123  ", "abc123")]
    [InlineData("not a code!", null)]
    [InlineData("", null)]
    public void ExtractsTheAuthorizationCode(string input, string? expected) => Assert.Equal(expected, EpicConnector.ExtractCode(input));

    [Fact]
    public async Task ExchangesCodeWithBasicAuthAndImportsGamesOnly()
    {
        var catalogCalls = 0;
        var handler = new StubHandler((request, _) =>
        {
            var url = request.RequestUri!;
            if (url.AbsolutePath.EndsWith("/oauth/token", StringComparison.Ordinal))
            {
                return StubHandler.Json("""
                    { "access_token": "eg1~a", "refresh_token": "eg1~r", "expires_at": "2026-11-04T18:00:00.000Z",
                      "account_id": "acc1", "displayName": "Tiny" }
                    """);
            }

            if (url.AbsolutePath.EndsWith("/public/items", StringComparison.Ordinal))
            {
                return url.Query.Contains("cursor=c2", StringComparison.Ordinal)
                    ? StubHandler.Json("""{ "records": [ { "namespace": "ns2", "catalogItemId": "i3", "appName": "Dlc" } ], "responseMetadata": {} }""")
                    : StubHandler.Json("""
                        { "records": [
                            { "namespace": "ns1", "catalogItemId": "i1", "appName": "Sugar" },
                            { "namespace": "ue", "catalogItemId": "u1", "appName": "UeAsset" } ],
                          "responseMetadata": { "nextCursor": "c2" } }
                        """);
            }

            if (url.AbsolutePath.Contains("/namespace/ns1/", StringComparison.Ordinal))
            {
                // Rate limited once: the connector retries instead of falling back to the app name.
                return ++catalogCalls == 1
                    ? StubHandler.Text("slow down", HttpStatusCode.TooManyRequests)
                    : StubHandler.Json("""
                        { "i1": { "title": "Rocket League", "keyImages": [
                            { "type": "OfferImageWide", "url": "https://cdn1.epicgames.com/wide.jpg" },
                            { "type": "DieselGameBoxTall", "url": "https://cdn1.epicgames.com/tall.jpg" } ] } }
                        """);
            }

            if (url.AbsolutePath.Contains("/namespace/ns2/", StringComparison.Ordinal))
            {
                return StubHandler.Json("""{ "i3": { "title": "Some DLC", "mainGameItem": { "id": "i1" } } }""");
            }

            return url.AbsolutePath.Contains("/playtime/", StringComparison.Ordinal)
                ? StubHandler.Json("""[ { "artifactId": "Sugar", "totalTime": 7200 } ]""")
                : StubHandler.Text("", HttpStatusCode.NotFound);
        });
        var epic = new EpicConnector(new HttpClient(handler), TestConfig.Default.Epic) { RetryDelay = TimeSpan.Zero };

        var start = epic.LoginStart("unused", "unused");
        Assert.Equal("https://www.epicgames.com/id/api/redirect", start.CallbackPrefix);
        Assert.NotNull(start.FallbackUrl);

        var session = await epic.LoginCompleteAsync(new LoginCompleteRequest(null, """{"authorizationCode":"xyz"}""", null), new StoreContext(null), TestContext.Current.CancellationToken);
        Assert.Equal(("acc1", "Tiny"), (session.AccountId, session.DisplayName));
        var tokenRequest = handler.Requests[0];
        Assert.Equal("Basic", tokenRequest.Request.Headers.Authorization!.Scheme);
        Assert.Contains("grant_type=authorization_code&code=xyz&token_type=eg1", tokenRequest.Body, StringComparison.Ordinal);

        var games = await epic.GetLibraryAsync(session, new StoreContext(null), TestContext.Current.CancellationToken);
        var game = Assert.Single(games);
        Assert.Equal(("Sugar", "Rocket League", 120L), (game.StoreGameId, game.Title, game.PlaytimeMinutes!.Value));
        Assert.Equal("https://cdn1.epicgames.com/tall.jpg?h=900&resize=1&w=600", game.ImageUrl);
        Assert.Equal(2, catalogCalls);
    }

    [Fact]
    public async Task LargeLibrariesAskTheCatalogueSeveralAtATime()
    {
        // One namespace per game, as with Epic's free games: one request after another took minutes.
        const int count = 40;
        var inFlight = 0;
        var mostAtOnce = 0;
        var handler = new AsyncStubHandler(async request =>
        {
            var url = request.RequestUri!;
            if (url.AbsolutePath.EndsWith("/public/items", StringComparison.Ordinal))
            {
                // Two games per namespace (editions, bundles): the catalogue gets several ids at once.
                var records = string.Join(',', Enumerable.Range(0, count).Select(i =>
                    $$"""{ "namespace": "ns{{i / 2}}", "catalogItemId": "item{{i}}", "appName": "app{{i}}" }"""));
                return StubHandler.Json($$"""{ "records": [ {{records}} ], "responseMetadata": {} }""");
            }

            if (url.AbsolutePath.Contains("/namespace/", StringComparison.Ordinal))
            {
                var now = Interlocked.Increment(ref inFlight);
                InterlockedMax(ref mostAtOnce, now);
                await Task.Delay(20).ConfigureAwait(false);
                Interlocked.Decrement(ref inFlight);
                // Like Epic: only repeated "id=" parameters count; a comma-separated list finds nothing.
                var ids = url.Query.TrimStart('?').Split('&')
                    .Where(p => p.StartsWith("id=", StringComparison.Ordinal) && !p.Contains(',', StringComparison.Ordinal))
                    .Select(p => p[3..]);
                return StubHandler.Json("{" + string.Join(',', ids.Select(id => $"\"{id}\": {{ \"title\": \"Game {id}\" }}")) + "}");
            }

            return StubHandler.Text("", HttpStatusCode.NotFound);
        });
        var epic = new EpicConnector(new HttpClient(handler), TestConfig.Default.Epic) { RetryDelay = TimeSpan.Zero };
        var session = new StoreSession("epic", "acc1", "Tiny", "eg1~a", "eg1~r", DateTimeOffset.UtcNow.AddHours(1));

        var games = await epic.GetLibraryAsync(session, new StoreContext(null), TestContext.Current.CancellationToken);

        Assert.Equal(count, games.Count);
        Assert.All(games, g => Assert.Equal($"Game item{g.StoreGameId[3..]}", g.Title));
        Assert.InRange(mostAtOnce, 2, 8);
        Assert.Equal(count / 2, handler.Calls);
    }

    private static void InterlockedMax(ref int target, int value)
    {
        int seen;
        while ((seen = Volatile.Read(ref target)) < value && Interlocked.CompareExchange(ref target, value, seen) != seen)
        {
        }
    }
}

/// <summary>Tests that read or change the process-wide UI language never run in parallel.</summary>
[CollectionDefinition("Language", DisableParallelization = true)]
public sealed class LanguageCollection;

[Collection("Language")]
public class FailureTests
{
    [Fact]
    public async Task NetworkAndServerErrorsBecomeStableKinds()
    {
        var offline = new GogConnector(new HttpClient(new StubHandler((_, _) => throw new HttpRequestException("down"))), TestConfig.Default.Gog);
        var network = await Assert.ThrowsAsync<ConnectorException>(() =>
            offline.LoginCompleteAsync(new LoginCompleteRequest(null, "c", null), new StoreContext(null), TestContext.Current.CancellationToken));
        Assert.Equal(ConnectorErrorKind.Network, network.Kind);

        var broken = new EpicConnector(new HttpClient(new StubHandler((_, _) => StubHandler.Text("oops", HttpStatusCode.BadGateway))), TestConfig.Default.Epic);
        var unavailable = await Assert.ThrowsAsync<ConnectorException>(() =>
            broken.LoginCompleteAsync(new LoginCompleteRequest(null, "abc", null), new StoreContext(null), TestContext.Current.CancellationToken));
        Assert.Equal(ConnectorErrorKind.Unavailable, unavailable.Kind);
        Assert.Contains("Epic ist gerade nicht verfügbar", unavailable.Message, StringComparison.Ordinal);

        // The store API changed: a required field is missing.
        var changed = new GogConnector(new HttpClient(new StubHandler((_, _) => StubHandler.Json("""{ "token": "x" }"""))), TestConfig.Default.Gog);
        var shape = await Assert.ThrowsAsync<ConnectorException>(() =>
            changed.LoginCompleteAsync(new LoginCompleteRequest(null, "c", null), new StoreContext(null), TestContext.Current.CancellationToken));
        Assert.Equal(ConnectorErrorKind.Unavailable, shape.Kind);
    }
}
