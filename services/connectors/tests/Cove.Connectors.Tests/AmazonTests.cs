using System.Net;
using System.Text.Json.Nodes;
using Cove.Connectors.Connectors;
using Cove.Connectors.Model;
using Xunit;

namespace Cove.Connectors.Tests;

public class AmazonTests
{
    private static readonly StoreSession Session = new("amazon", "SERIAL123", "Alex", "atoken", "rtoken", null);

    private static StubHandler Amazon() => new((request, body) =>
    {
        var url = request.RequestUri!.ToString();
        var target = request.Headers.TryGetValues("X-Amz-Target", out var values) ? values.First() : "";
        return url switch
        {
            _ when url.EndsWith("/auth/register", StringComparison.Ordinal) => StubHandler.Json("""
                { "response": { "success": {
                  "tokens": { "bearer": { "access_token": "Atza|a", "refresh_token": "Atzr|r", "expires_in": "3600" } },
                  "extensions": { "customer_info": { "given_name": "Alex", "user_id": "amzn1.account.X" },
                                  "device_info": { "device_serial_number": "DEV42" } } } } }
                """),
            _ when url.EndsWith("/auth/token", StringComparison.Ordinal) =>
                StubHandler.Json("""{ "access_token": "Atza|new", "expires_in": 3600 }"""),
            _ when url.EndsWith("/distribution/entitlements", StringComparison.Ordinal) && !body.Contains("\"nextToken\":\"p2\"", StringComparison.Ordinal) =>
                StubHandler.Json("""
                    { "entitlements": [
                      { "id": "ent-1", "product": { "id": "amzn1.adg.product.1", "title": "Tidewater", "sku": "SKU1" } },
                      { "id": "ent-1b", "product": { "id": "amzn1.adg.product.1", "title": "Tidewater" } } ],
                      "nextToken": "p2" }
                    """),
            _ when url.EndsWith("/distribution/entitlements", StringComparison.Ordinal) =>
                StubHandler.Json("""
                    { "entitlements": [ { "id": "ent-2", "product": { "id": "amzn1.adg.product.2", "title": "Paper Orbit",
                      "productDetail": { "iconUrl": "https://img.test/orbit.png" } } } ] }
                    """),
            _ when target.EndsWith(".GetGameDownload", StringComparison.Ordinal) && body.Contains("ent-2", StringComparison.Ordinal) =>
                StubHandler.Json("""{ "downloadUrl": "https://cdn.test/v/7?sig=1", "versionId": "v7" }"""),
            _ when target.EndsWith(".GetLiveVersionIds", StringComparison.Ordinal) =>
                StubHandler.Json("""{ "adgProductIdToVersionIdMap": { "amzn1.adg.product.2": "v8" } }"""),
            _ when url.Contains("/download/channel/", StringComparison.Ordinal) =>
                StubHandler.Json("""{ "downloadUrl": "https://cdn.test/sdk", "versionId": "s1" }"""),
            _ => StubHandler.Text("", HttpStatusCode.NotFound),
        };
    });

    [Fact]
    public void LoginUsesPkceAndTheDeviceClient()
    {
        var connector = new AmazonConnector(new HttpClient(Amazon()), TestConfig.Default.Amazon);
        var start = connector.LoginStart("", "state-1");
        Assert.Equal(LoginMode.Webview, start.Mode);
        Assert.Equal("https://www.amazon.com/?openid", start.CallbackPrefix);
        var query = start.Url.Split('?')[1].Split('&')
            .Select(p => p.Split('=', 2))
            .ToDictionary(p => Uri.UnescapeDataString(p[0]), p => Uri.UnescapeDataString(p[1]));
        Assert.Equal("S256", query["openid.oa2.code_challenge_method"]);
        Assert.Equal("amzn_sonic_games_launcher", query["openid.assoc_handle"]);
        var serial = AmazonConnector.Serial("state-1");
        Assert.Equal(32, serial.Length);
        Assert.Equal("device:" + connector.ClientId(serial), query["openid.oa2.client_id"]);
        // client id = hex of "<serial>#<device type>"
        Assert.EndsWith(Convert.ToHexStringLower("#A2UMVHOX7UP4V7"u8.ToArray()), connector.ClientId(serial), StringComparison.Ordinal);
        // The same state gives the same verifier; another state another one.
        Assert.Equal(AmazonConnector.Verifier("state-1"), AmazonConnector.Verifier("state-1"));
        Assert.NotEqual(AmazonConnector.Verifier("state-1"), AmazonConnector.Verifier("state-2"));
        Assert.Equal(43, AmazonConnector.Verifier("state-1").Length);
    }

    [Fact]
    public async Task RegistersTheDeviceAndRefreshes()
    {
        var handler = Amazon();
        var connector = new AmazonConnector(new HttpClient(handler), TestConfig.Default.Amazon);
        var ct = TestContext.Current.CancellationToken;
        var session = await connector.LoginCompleteAsync(new LoginCompleteRequest(null, " CODE ", "state-1"), new StoreContext(null), ct);
        Assert.Equal(("DEV42", "Alex", "Atza|a", "Atzr|r"), (session.AccountId, session.DisplayName, session.AccessToken, session.RefreshToken));
        var body = JsonNode.Parse(handler.Requests[0].Body)!;
        Assert.Equal("CODE", body["auth_data"]!["authorization_code"]!.GetValue<string>());
        Assert.Equal(AmazonConnector.Verifier("state-1"), body["auth_data"]!["code_verifier"]!.GetValue<string>());
        Assert.Equal(AmazonConnector.Serial("state-1"), body["registration_data"]!["device_serial"]!.GetValue<string>());

        var refreshed = await connector.RefreshAsync(session, ct);
        Assert.Equal(("Atza|new", "Atzr|r", "DEV42"), (refreshed.AccessToken, refreshed.RefreshToken, refreshed.AccountId));

        await Assert.ThrowsAsync<ConnectorException>(() =>
            connector.LoginCompleteAsync(new LoginCompleteRequest(null, "CODE", null), new StoreContext(null), ct));
    }

    [Fact]
    public async Task LibraryPagesAndDownloadsGoThroughTheDistributionService()
    {
        var handler = Amazon();
        var connector = new AmazonConnector(new HttpClient(handler), TestConfig.Default.Amazon);
        var ct = TestContext.Current.CancellationToken;
        var games = await connector.GetLibraryAsync(Session, new StoreContext(null), ct);
        Assert.Equal(["Tidewater", "Paper Orbit"], games.Select(g => g.Title));
        Assert.Equal("https://img.test/orbit.png", games[1].ImageUrl);
        var (request, body) = handler.Requests[0];
        Assert.Equal("atoken", request.Headers.GetValues("x-amzn-token").Single());
        Assert.Equal("amz-1.0", request.Content!.Headers.ContentEncoding.Single());
        var hash = Convert.ToHexString(System.Security.Cryptography.SHA256.HashData("SERIAL123"u8.ToArray()));
        Assert.Equal(hash, JsonNode.Parse(body)!["hardwareHash"]!.GetValue<string>());

        var content = new AmazonContent(new HttpClient(handler), connector, TestConfig.Default.Amazon);
        var download = await content.DownloadAsync(Session, "amzn1.adg.product.2", ct);
        Assert.Equal(("ent-2", "v7", "https://cdn.test/v/7?sig=1"), (download.EntitlementId, download.VersionId, download.DownloadUrl));
        var versions = await content.VersionsAsync(Session, ["amzn1.adg.product.2"], ct);
        Assert.Equal("v8", versions.Versions["amzn1.adg.product.2"]);
        var sdk = await content.SdkAsync(Session, ct);
        Assert.Equal("https://cdn.test/sdk", sdk.DownloadUrl);
        await Assert.ThrowsAsync<ConnectorException>(() => content.DownloadAsync(Session, "amzn1.adg.product.9", ct));
    }
}
