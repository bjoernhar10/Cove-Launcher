using System.Net;
using Cove.Connectors.Connectors;
using Cove.Connectors.Model;
using Xunit;

namespace Cove.Connectors.Tests;

public class GogContentTests
{
    private static readonly StoreSession Session = new("gog", "1", "x", "a1", "r1", null);

    private const string BuildManifest = """
        {
          "version": 2, "baseProductId": "100", "installDirectory": "Saltmarsh",
          "products": [ { "productId": "100", "name": "Saltmarsh" }, { "productId": "200", "name": "Saltmarsh: Deep Water" },
                        { "productId": "300", "name": "Not owned" } ],
          "depots": [
            { "productId": "100", "languages": ["*"], "manifest": "aaaa1111", "size": 10, "compressedSize": 8 },
            { "productId": "100", "languages": ["de-DE"], "manifest": "dede2222", "size": 3, "compressedSize": 3 },
            { "productId": "100", "languages": ["en-US"], "manifest": "enen3333", "size": 3, "compressedSize": 3 },
            { "productId": "200", "languages": ["*"], "manifest": "d1c04444", "size": 1, "compressedSize": 1 },
            { "productId": "300", "languages": ["*"], "manifest": "0ff05555", "size": 1, "compressedSize": 1 }
          ],
          "offlineDepot": { "productId": "100", "languages": ["*"], "manifest": "0ff1ce66", "size": 1, "compressedSize": 1 }
        }
        """;

    private static HttpResponseMessage Zlib(string json) =>
        new(HttpStatusCode.OK) { Content = new ByteArrayContent(GogContent.Compress(json)) };

    private static StubHandler Cdn(Func<string, HttpResponseMessage?>? extra = null) => new((request, _) =>
    {
        var url = request.RequestUri!.ToString();
        if (extra?.Invoke(url) is { } answer)
        {
            return answer;
        }

        return url switch
        {
            _ when url.Contains("/builds?generation=2", StringComparison.Ordinal) => StubHandler.Json("""
                { "items": [
                  { "build_id": "9", "version_name": "1.1 beta", "generation": 2, "branch": "beta", "link": "https://cdn.test/beta" },
                  { "build_id": "8", "version_name": "1.0", "generation": 2, "branch": null, "link": "https://cdn.test/build8" }
                ] }
                """),
            "https://cdn.test/build8" => Zlib(BuildManifest),
            _ when url.EndsWith("aa/aa/aaaa1111", StringComparison.Ordinal) => Zlib("""
                { "depot": {
                  "items": [
                    { "type": "DepotDirectory", "path": "bin" },
                    { "type": "DepotFile", "path": "bin\\Saltmarsh.exe", "flags": ["executable"],
                      "chunks": [ { "md5": "m1", "compressedMd5": "c1", "size": 6, "compressedSize": 5 },
                                  { "md5": "m2", "compressedMd5": "c2", "size": 4, "compressedSize": 3 } ] },
                    { "type": "DepotFile", "path": "tiny.txt", "sfcRef": { "offset": 2, "size": 2 } },
                    { "type": "DepotLink", "path": "start", "target": "bin\\Saltmarsh.exe" }
                  ],
                  "smallFilesContainer": { "chunks": [ { "md5": "s1", "compressedMd5": "sc1", "size": 4, "compressedSize": 4 } ] }
                } }
                """),
            _ when url.EndsWith("de/de/dede2222", StringComparison.Ordinal) => Zlib("""
                { "depot": { "items": [ { "type": "DepotFile", "path": "lang/de.pak", "chunks": [ { "md5": "d", "compressedMd5": "dc", "size": 3, "compressedSize": 3 } ] } ] } }
                """),
            _ when url.EndsWith("en/en/enen3333", StringComparison.Ordinal) => Zlib("""
                { "depot": { "items": [ { "type": "DepotFile", "path": "lang/en.pak", "chunks": [ { "md5": "e", "compressedMd5": "ec", "size": 3, "compressedSize": 3 } ] } ] } }
                """),
            _ when url.EndsWith("d1/c0/d1c04444", StringComparison.Ordinal) => Zlib("""
                { "depot": { "items": [ { "type": "DepotFile", "path": "dlc/extra.pak", "chunks": [ { "md5": "x", "compressedMd5": "xc", "size": 1, "compressedSize": 1 } ] } ] } }
                """),
            _ when url.EndsWith("0f/f1/0ff1ce66", StringComparison.Ordinal) => Zlib("""
                { "depot": { "items": [ { "type": "DepotFile", "path": "goggame-100.info", "chunks": [ { "md5": "i", "compressedMd5": "ic", "size": 1, "compressedSize": 1 } ] } ] } }
                """),
            "https://embed.gog.com/user/data/games" => StubHandler.Json("""{ "owned": [100, 200] }"""),
            _ when url.Contains("/secure_link", StringComparison.Ordinal) => StubHandler.Json("""
                { "urls": [
                  { "endpoint_name": "slow", "priority": 1, "url_format": "{base_url}/slow{path}", "parameters": { "base_url": "https://slow.test", "path": "/store/x" } },
                  { "endpoint_name": "fast", "priority": 9, "url_format": "{base_url}/token=nva={expires_at}~token={token}{path}",
                    "parameters": { "base_url": "https://fast.test", "path": "/content-system/v2/store/100/", "expires_at": 1700000000, "token": "t0k" } }
                ] }
                """),
            _ => StubHandler.Text("", HttpStatusCode.NotFound),
        };
    });

    [Fact]
    public async Task BuildsAPlanFromTheNewestPublicBuild()
    {
        var handler = Cdn();
        var gog = new GogContent(new HttpClient(handler), TestConfig.Default.Gog);
        var plan = await gog.PlanAsync(Session, "100", "windows", "de", TestContext.Current.CancellationToken);

        Assert.Equal(("8", "1.0", "windows"), (plan.BuildId, plan.Version, plan.Platform));
        // German instead of English, owned DLC 200 but not 300, offline depot included.
        Assert.Equal(
            ["bin/Saltmarsh.exe", "tiny.txt", "lang/de.pak", "dlc/extra.pak", "goggame-100.info"],
            plan.Files.Select(f => f.Path));
        Assert.True(plan.Files[0].Executable);
        Assert.Equal(new GogSfcRef(0, 2, 2), plan.Files[1].Sfc);
        Assert.Empty(plan.Files[1].Chunks);
        Assert.Equal(["bin"], plan.Directories);
        Assert.Equal(new GogLink("start", "bin/Saltmarsh.exe"), plan.Links.Single());
        Assert.Equal(8 + 4 + 3 + 1 + 1, plan.DownloadBytes);
        Assert.Equal(10 + 2 + 3 + 1 + 1, plan.InstallBytes);
        Assert.Equal(
            ["https://fast.test/token=nva=1700000000~token=t0k/content-system/v2/store/100/{chunk}", "https://slow.test/slow/store/x/{chunk}"],
            plan.ChunkUrls["100"]);
        Assert.Contains("200", plan.ChunkUrls.Keys);
        Assert.Equal([new GogDlc("200", "Saltmarsh: Deep Water")], plan.Dlcs);
        Assert.DoesNotContain(handler.Requests, r => r.Request.RequestUri!.ToString().EndsWith("0ff05555", StringComparison.Ordinal));
        Assert.All(
            handler.Requests.Where(r => r.Request.RequestUri!.Host == "content-system.gog.com"),
            r => Assert.Equal("a1", r.Request.Headers.Authorization!.Parameter));
    }

    [Fact]
    public async Task ListsOwnedDlcsAndMarksTheOnesWithoutFiles()
    {
        var gog = new GogContent(new HttpClient(Cdn(url => url switch
        {
            "https://embed.gog.com/user/data/games" => StubHandler.Json("""{ "owned": [100, 200, 400] }"""),
            "https://api.gog.com/products/100?expand=expanded_dlcs" => StubHandler.Json("""
                { "id": 100, "expanded_dlcs": [ { "id": 200, "title": "Saltmarsh: Deep Water" }, { "id": 300, "title": "Not owned" },
                                                { "id": 400, "title": "Saltmarsh: Golden Boat" } ] }
                """),
            _ => null,
        })), TestConfig.Default.Gog);
        var owned = await gog.OwnedDlcsAsync(Session, "100", "windows", TestContext.Current.CancellationToken);
        // 200 has a depot in the build; 400 is only unlocked in the game.
        Assert.Equal([new OwnedDlc("200", "Saltmarsh: Deep Water", false), new OwnedDlc("400", "Saltmarsh: Golden Boat", true)], owned.Dlcs);
    }

    [Fact]
    public async Task FallsBackToEnglishAndRefusesTheOldFormat()
    {
        var gog = new GogContent(new HttpClient(Cdn()), TestConfig.Default.Gog);
        var english = await gog.PlanAsync(Session, "100", "windows", "fr", TestContext.Current.CancellationToken);
        Assert.DoesNotContain(english.Files, f => f.Path == "lang/de.pak");
        Assert.Contains(english.Files, f => f.Path == "lang/en.pak");

        var old = new GogContent(new HttpClient(Cdn(url => url.Contains("/builds", StringComparison.Ordinal)
            ? StubHandler.Json("""{ "items": [ { "build_id": "1", "generation": 1, "link": "https://cdn.test/v1" } ] }""")
            : null)), TestConfig.Default.Gog);
        var error = await Assert.ThrowsAsync<ConnectorException>(() => old.PlanAsync(Session, "100", "windows", "de", TestContext.Current.CancellationToken));
        Assert.Contains("GOG Galaxy", error.Message, StringComparison.Ordinal);

        await Assert.ThrowsAsync<ConnectorException>(() => gog.PlanAsync(Session, "../x", "windows", "de", TestContext.Current.CancellationToken));
    }

    [Fact]
    public async Task NativeLinuxGamesUseGogsInstaller()
    {
        var handler = Cdn(url => url switch
        {
            "https://api.gog.com/products/100?expand=downloads" => StubHandler.Json("""
                { "downloads": { "installers": [
                  { "os": "windows", "language": "en", "version": "1.0", "files": [ { "size": 9, "downlink": "https://api.gog.com/win" } ] },
                  { "os": "linux", "language": "en", "version": "1.0.2", "total_size": 5000,
                    "files": [ { "size": 5000, "downlink": "https://api.gog.com/products/100/downlink/installer/en3installer0" } ] }
                ] } }
                """),
            "https://api.gog.com/products/100/downlink/installer/en3installer0" =>
                StubHandler.Json("""{ "downlink": "https://cdn.test/secure/saltmarsh_1_0_2.sh?token=x" }"""),
            _ => null,
        });
        var gog = new GogContent(new HttpClient(handler), TestConfig.Default.Gog);
        var plan = await gog.PlanAsync(Session, "100", "linux", "de", TestContext.Current.CancellationToken);
        Assert.Empty(plan.Dlcs);
        Assert.Equal("linux", plan.Platform);
        Assert.Equal(new GogInstaller("https://cdn.test/secure/saltmarsh_1_0_2.sh?token=x", "saltmarsh_1_0_2.sh", 5000), plan.Installer);
        Assert.Equal("installer:1.0.2", plan.BuildId);
        Assert.Empty(plan.Files);

        var latest = await gog.LatestAsync(Session, "100", "linux", "de", TestContext.Current.CancellationToken);
        Assert.Equal(new GogLatest("installer:1.0.2", "1.0.2", "linux"), latest);
        var windows = await gog.LatestAsync(Session, "100", "windows", "de", TestContext.Current.CancellationToken);
        Assert.Equal(new GogLatest("8", "1.0", "windows"), windows);
    }

    [Fact]
    public void PathsStayInsideTheGameFolder()
    {
        Assert.Equal("bin/game.exe", GogContent.CleanPath("\\bin\\game.exe"));
        Assert.Throws<ConnectorException>(() => GogContent.CleanPath("../evil"));
        Assert.Throws<ConnectorException>(() => GogContent.CleanPath("C:\\Windows\\x"));
        Assert.Throws<ConnectorException>(() => GogContent.CleanPath("a//b"));
        Assert.Equal("ab/cd/abcdef", GogContent.GalaxyPath("abcdef"));
    }
}
