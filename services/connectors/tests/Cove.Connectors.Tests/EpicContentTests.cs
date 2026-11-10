using System.Net;
using Cove.Connectors.Connectors;
using Cove.Connectors.Model;
using Xunit;

namespace Cove.Connectors.Tests;

public class EpicContentTests
{
    private static readonly StoreSession Session = new("epic", "acc1", "Tiny", "a1", "r1", null);

    private static StubHandler Epic(string attributes) => new((request, _) =>
    {
        var url = request.RequestUri!.ToString();
        return url switch
        {
            _ when url.Contains("/library/api/public/items", StringComparison.Ordinal) && !url.Contains("cursor", StringComparison.Ordinal) =>
                StubHandler.Json("""{ "records": [ { "namespace": "ns0", "catalogItemId": "other", "appName": "Other" } ], "responseMetadata": { "nextCursor": "c2" } }"""),
            _ when url.Contains("/library/api/public/items", StringComparison.Ordinal) =>
                StubHandler.Json("""
                    { "records": [ { "namespace": "ns1", "catalogItemId": "cat1", "appName": "Sugar" },
                      { "namespace": "ns1", "catalogItemId": "dlcFiles", "appName": "SugarIsland" },
                      { "namespace": "ns1", "catalogItemId": "dlcUnlock", "appName": "SugarSkin" },
                      { "namespace": "ns1", "catalogItemId": "dlcMac", "appName": "SugarMac" } ] }
                    """),
            _ when url.Contains("/catalogItem/dlcFiles/app/SugarIsland/label/Live", StringComparison.Ordinal) =>
                StubHandler.Json("""
                    { "elements": [ { "appName": "SugarIsland", "buildVersion": "7-Windows", "manifests": [
                      { "uri": "https://cdn.test/Builds/Org/o/island/default/i.manifest" } ] } ] }
                    """),
            _ when url.Contains("/assets/v2/platform/Windows/namespace/ns1/catalogItem/cat1/app/Sugar/label/Live", StringComparison.Ordinal) =>
                StubHandler.Json("""
                    { "elements": [ { "appName": "Sugar", "buildVersion": "1.2.3-Windows", "manifests": [
                      { "uri": "https://cdn.test/Builds/Org/o/x/default/abc.manifest", "queryParams": [ { "name": "f_token", "value": "a b" } ] },
                      { "uri": "https://cdn2.test/Builds/Org/o/x/default/abc.manifest", "queryParams": [] },
                      { "uri": "http://insecure.test/abc.manifest" } ] } ] }
                    """),
            _ when url.Contains("/bulk/items", StringComparison.Ordinal) => StubHandler.Json($$"""
                { "cat1": { "title": "Sugar", "customAttributes": {{attributes}}, "dlcItemList": [
                  { "id": "dlcFiles", "title": "Sugar Island", "releaseInfo": [ { "appId": "SugarIsland", "platform": [ "Windows" ] } ] },
                  { "id": "dlcUnlock", "title": "Golden Skin" },
                  { "id": "dlcMac", "title": "Mac Only", "releaseInfo": [ { "appId": "SugarMac", "platform": [ "Mac" ] } ] },
                  { "id": "dlcNotOwned", "title": "Not Owned" } ] } }
                """),
            _ when url.EndsWith("/oauth/exchange", StringComparison.Ordinal) => StubHandler.Json("""{ "code": "ex-123" }"""),
            _ when url.Contains("/ownershipToken", StringComparison.Ordinal) =>
                new HttpResponseMessage(HttpStatusCode.OK) { Content = new ByteArrayContent([0, 1, 2, 255]) },
            _ when url.Contains("/savesync/acc1/Sugar/", StringComparison.Ordinal) && request.Method == HttpMethod.Get => StubHandler.Json("""
                { "files": {
                  "acc1/Sugar/manifests/2026.11.01-10.00.00.manifest": { "readLink": "https://saves.test/m1", "lastModified": "2026-11-01T10:00:00.000Z", "size": 120 },
                  "acc1/Sugar/ChunksV4/00/AB_CD.chunk": { "readLink": "https://saves.test/c1", "lastModified": "2026-11-01T10:00:00.000Z", "size": 300 } } }
                """),
            _ when url.Contains("/savesync/acc1/Sugar/", StringComparison.Ordinal) => StubHandler.Json("""
                { "files": { "acc1/Sugar/manifests/new.manifest": { "writeLink": "https://saves.test/w1" } } }
                """),
            _ => StubHandler.Text("", HttpStatusCode.NotFound),
        };
    });

    [Fact]
    public async Task FindsManifestsAndCatalogueAttributes()
    {
        var handler = Epic("""{ "CloudSaveFolder": { "type": "STRING", "value": "{AppData}/Sugar/Saved" }, "OwnershipToken": { "type": "STRING", "value": "true" } }""");
        var epic = new EpicContent(new HttpClient(handler), TestConfig.Default.Epic);
        var info = await epic.InstallInfoAsync(Session, "Sugar", "windows", TestContext.Current.CancellationToken);
        Assert.Equal(("ns1", "cat1", "1.2.3-Windows"), (info.Namespace, info.CatalogItemId, info.BuildVersion));
        Assert.Equal(
            ["https://cdn.test/Builds/Org/o/x/default/abc.manifest?f_token=a%20b", "https://cdn2.test/Builds/Org/o/x/default/abc.manifest"],
            info.ManifestUrls);
        Assert.Null(info.LauncherOnly);
        Assert.Equal("{AppData}/Sugar/Saved", info.CloudSaveFolder);
        Assert.True(info.OwnershipToken);
        // Only owned DLCs with a build of their own; "Golden Skin" just unlocks content (404 on assets).
        var dlc = Assert.Single(info.Dlcs);
        Assert.Equal(("SugarIsland", "dlcFiles", "Sugar Island", "7-Windows"), (dlc.AppName, dlc.CatalogItemId, dlc.Title, dlc.BuildVersion));
        Assert.Equal(["https://cdn.test/Builds/Org/o/island/default/i.manifest"], dlc.ManifestUrls);
        Assert.DoesNotContain(handler.Requests, r => r.Request.RequestUri!.ToString().Contains("SugarMac", StringComparison.Ordinal));
        Assert.All(handler.Requests, r => Assert.Equal("a1", r.Request.Headers.Authorization!.Parameter));
    }

    [Fact]
    public async Task GamesOfOtherLaunchersStayWithTheEpicLauncher()
    {
        var epic = new EpicContent(new HttpClient(Epic("""{ "ThirdPartyManagedApp": { "type": "STRING", "value": "the EA app" } }""")), TestConfig.Default.Epic);
        var info = await epic.InstallInfoAsync(Session, "Sugar", "windows", TestContext.Current.CancellationToken);
        Assert.Contains("Epic Launcher", info.LauncherOnly, StringComparison.Ordinal);
        Assert.Contains("the EA app", info.LauncherOnly, StringComparison.Ordinal);
        Assert.Empty(info.Dlcs);
        await Assert.ThrowsAsync<ConnectorException>(() => epic.InstallInfoAsync(Session, "Unknown", "windows", TestContext.Current.CancellationToken));
        await Assert.ThrowsAsync<ConnectorException>(() => epic.InstallInfoAsync(Session, "../x", "windows", TestContext.Current.CancellationToken));
    }

    [Fact]
    public async Task StartsWithExchangeCodeAndOwnershipToken()
    {
        var handler = Epic("{}");
        var epic = new EpicContent(new HttpClient(handler), TestConfig.Default.Epic);
        var launch = await epic.LaunchAsync(Session, "Sugar", ownershipToken: true, TestContext.Current.CancellationToken);
        Assert.Equal(new EpicLaunch("ex-123", "acc1", "Tiny", "AAEC/w=="), launch);
        var token = handler.Requests.Single(r => r.Request.RequestUri!.AbsolutePath.EndsWith("/ownershipToken", StringComparison.Ordinal));
        Assert.Equal("nsCatalogItemId=ns1%3Acat1", token.Body);
        Assert.Contains("/identities/acc1/", token.Request.RequestUri!.AbsolutePath, StringComparison.Ordinal);

        var plain = await epic.LaunchAsync(Session, "Sugar", ownershipToken: false, TestContext.Current.CancellationToken);
        Assert.Null(plain.OwnershipToken);
    }

    [Fact]
    public async Task ListsSavesAndHandsOutUploadLinks()
    {
        var epic = new EpicContent(new HttpClient(Epic("{}")), TestConfig.Default.Epic);
        var saves = await epic.SavesAsync(Session, "Sugar", TestContext.Current.CancellationToken);
        Assert.Equal(2, saves.Files.Count);
        Assert.Equal(new DateTimeOffset(2026, 11, 1, 10, 0, 0, TimeSpan.Zero), saves.Files[0].LastModified);
        Assert.Equal("https://saves.test/m1", saves.Files[0].ReadLink);

        var links = await epic.WriteLinksAsync(Session, "Sugar", ["acc1/Sugar/manifests/new.manifest"], TestContext.Current.CancellationToken);
        Assert.Equal("https://saves.test/w1", links.Links["acc1/Sugar/manifests/new.manifest"]);
        await Assert.ThrowsAsync<ConnectorException>(() => epic.WriteLinksAsync(Session, "Sugar", ["other/Sugar/x"], TestContext.Current.CancellationToken));
        await Assert.ThrowsAsync<ConnectorException>(() => epic.WriteLinksAsync(Session, "Sugar", ["acc1/Sugar/../x"], TestContext.Current.CancellationToken));
    }
}
