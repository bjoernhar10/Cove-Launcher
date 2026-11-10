using System.Net;
using Cove.Connectors.Connectors;
using Cove.Connectors.Model;
using Xunit;

namespace Cove.Connectors.Tests;

public class SteamContentTests
{
    [Fact]
    public async Task ListsDlcsWithNamesAndMarksInstalledOnes()
    {
        using var tree = new TempTree();
        tree.Write(".steam/steam/steamapps/libraryfolders.vdf", $$"""
            "libraryfolders" { "0" { "path" "{{Path.Combine(tree.Root, ".steam", "steam")}}" } }
            """);
        tree.Write(".steam/steam/steamapps/appmanifest_620.acf", """
            "AppState"
            {
                "appid" "620"
                "InstalledDepots"
                {
                    "621" { "manifest" "1" "size" "10" }
                    "650" { "manifest" "2" "size" "5" "dlcappid" "651" }
                }
            }
            """);
        var handler = new StubHandler((request, _) =>
        {
            var url = Uri.UnescapeDataString(request.RequestUri!.ToString());
            return url switch
            {
                _ when url.Contains("/api/appdetails?appids=620", StringComparison.Ordinal) =>
                    StubHandler.Json("""{ "620": { "success": true, "data": { "name": "Portal 2", "dlc": [652, 651, 653] } } }"""),
                _ when url.Contains("IStoreBrowseService/GetItems", StringComparison.Ordinal) && url.Contains("\"appid\":651", StringComparison.Ordinal) =>
                    StubHandler.Json("""{ "response": { "store_items": [ { "appid": 651, "name": "Peer Review" }, { "appid": 652, "name": "Art Pack" } ] } }"""),
                _ => StubHandler.Text("", HttpStatusCode.NotFound),
            };
        });
        var steam = new SteamContent(new HttpClient(handler), TestConfig.Default.Steam);

        var result = await steam.DlcsAsync("620", new LocalEnvironment(tree.Root, null, IsWindows: false), TestContext.Current.CancellationToken);

        // Installed first; a DLC without a store name shows its id.
        Assert.Equal(
            [new SteamDlc("651", "Peer Review", true), new SteamDlc("652", "Art Pack", false), new SteamDlc("653", "DLC 653", false)],
            result.Dlcs);
        Assert.Equal(3, result.Total);
        await Assert.ThrowsAsync<ConnectorException>(() => steam.DlcsAsync("../1", new LocalEnvironment(tree.Root, null, false), TestContext.Current.CancellationToken));
    }
}
