using Cove.Connectors.Connectors;
using Cove.Connectors.Local;
using Xunit;

namespace Cove.Connectors.Tests;

public class LocalDetectionTests
{
    [Fact]
    public void ParsesKeyValuesWithCommentsEscapesAndNesting()
    {
        var kv = KeyValues.Parse("""
            // comment
            "AppState"
            {
                "appid"     "105600"
                "name"      "Terraria \"Journey\""
                "UserConfig" { "language" "german" }
            }
            """);
        Assert.Equal("105600", kv["AppState"]!.Get("APPID"));
        Assert.Equal("Terraria \"Journey\"", kv["appstate"]!.Get("name"));
        Assert.Equal("german", kv["AppState"]!["UserConfig"]!.Get("language"));
        Assert.Throws<FormatException>(() => KeyValues.Parse("\"a\" { \"b\" \"c\""));
    }

    private static string Acf(string appId, string name, string dir, int flags = 4, long size = 1000) => $$"""
        "AppState"
        {
            "appid"      "{{appId}}"
            "name"       "{{name}}"
            "installdir" "{{dir}}"
            "StateFlags" "{{flags}}"
            "SizeOnDisk" "{{size}}"
        }
        """;

    [Fact]
    public void FindsInstalledSteamGamesInAllLibraries()
    {
        using var tree = new TempTree();
        var second = Path.Combine(tree.Root, "mnt", "games");
        tree.Write(".steam/steam/steamapps/libraryfolders.vdf", $$"""
            "libraryfolders"
            {
                "0" { "path" "{{Path.Combine(tree.Root, ".steam", "steam")}}" }
                "1" { "path" "{{second}}" }
            }
            """);
        tree.Write(".steam/steam/steamapps/appmanifest_105600.acf", Acf("105600", "Terraria", "Terraria", size: 470_000_000));
        tree.Write(".steam/steam/steamapps/appmanifest_1070560.acf", Acf("1070560", "Steam Linux Runtime 1.0 (scout)", "SteamLinuxRuntime"));
        tree.Write(".steam/steam/steamapps/appmanifest_2805730.acf", Acf("2805730", "Proton 9.0", "Proton 9.0"));
        tree.Write("mnt/games/steamapps/appmanifest_620.acf", Acf("620", "Portal 2", "Portal 2"));
        // Still downloading: StateFlags without the "installed" bit.
        tree.Write("mnt/games/steamapps/appmanifest_400.acf", Acf("400", "Portal", "Portal", flags: 1026));

        var games = LocalDetection.DetectSteam(new LocalEnvironment(tree.Root, null, IsWindows: false));

        Assert.Equal(["Portal 2", "Terraria"], games.Select(g => g.Title));
        var terraria = games.Single(g => g.StoreGameId == "105600");
        Assert.Equal("steam", terraria.Store);
        Assert.Equal(470_000_000, terraria.SizeBytes);
        Assert.EndsWith(Path.Combine("steamapps", "common", "Terraria"), terraria.InstallPath);
    }

    [Fact]
    public void FindsHeroicGogAndLegendaryGames()
    {
        using var tree = new TempTree();
        var witcher = Path.Combine(tree.Root, "Games", "The Witcher 3");
        tree.Write(".config/heroic/gog_store/installed.json", $$"""
            { "installed": [
                { "appName": "1207664663", "install_path": "{{witcher}}", "platform": "windows" },
                { "appName": "1207658924", "install_path": "{{Path.Combine(tree.Root, "Games", "untitled")}}" }
            ] }
            """);
        tree.Write(".config/heroic/store_cache/gog_library.json", """
            { "games": [ { "app_name": "1207664663", "title": "The Witcher 3: Wild Hunt" } ] }
            """);
        tree.Write("Games/untitled/gameinfo", "Beneath a Steel Sky\n1.0\n");
        tree.Write(".config/legendary/installed.json", $$"""
            {
              "Fortnite": { "title": "Fortnite", "install_path": "/games/Fortnite", "executable": "FortniteLauncher.exe", "install_size": 30000 },
              "SomeDlc": { "title": "DLC", "install_path": "/games/dlc", "is_dlc": true }
            }
            """);

        var env = new LocalEnvironment(tree.Root, null, IsWindows: false);
        var gog = LocalDetection.DetectHeroicGog(env);
        Assert.Equal(["The Witcher 3: Wild Hunt", "Beneath a Steel Sky"], gog.Select(g => g.Title));

        var epic = Assert.Single(LocalDetection.DetectLegendary(env));
        Assert.Equal(("epic", "Fortnite", 30000L), (epic.Store, epic.StoreGameId, epic.SizeBytes!.Value));
        Assert.Equal(Path.Combine("/games/Fortnite", "FortniteLauncher.exe"), epic.ExecutablePath);
    }

    [Fact]
    public void ReadsEpicLauncherManifestsAndSkipsBrokenFiles()
    {
        using var tree = new TempTree();
        var manifests = "ProgramData/Epic/EpicGamesLauncher/Data/Manifests";
        tree.Write($"{manifests}/A.item", """
            { "AppName": "Sugar", "DisplayName": "Rocket League", "InstallLocation": "C:\\Games\\RL",
              "LaunchExecutable": "Binaries\\RL.exe", "InstallSize": 25000000000 }
            """);
        tree.Write($"{manifests}/B.item", """{ "AppName": "Half", "DisplayName": "Half done", "InstallLocation": "C:\\x", "bIsIncompleteInstall": true }""");
        tree.Write($"{manifests}/C.item", "{ broken json");

        var games = LocalDetection.DetectEpicManifests(new LocalEnvironment(tree.Root, Path.Combine(tree.Root, "ProgramData"), IsWindows: true));
        var game = Assert.Single(games);
        Assert.Equal(("Sugar", "Rocket League", 25_000_000_000L), (game.StoreGameId, game.Title, game.SizeBytes!.Value));
    }

    [Fact]
    public void MissingFoldersMeanNoGames()
    {
        using var tree = new TempTree();
        Assert.Empty(LocalDetection.DetectAll(new LocalEnvironment(tree.Root, Path.Combine(tree.Root, "nope"), IsWindows: false)));
    }
}
