using System.Runtime.Versioning;
using System.Text.Json;
using System.Text.Json.Nodes;
using Cove.Connectors.Connectors;
using Cove.Connectors.Model;

namespace Cove.Connectors.Local;

/// <summary>
/// Finds installed games without any login: Steam manifests, Heroic/Legendary
/// configs (GOG + Epic on Linux), Heroic's Amazon installs (nile), Epic launcher
/// manifests and the GOG Galaxy and Amazon Games registry entries (Windows). Unreadable files are skipped, never fatal.
/// </summary>
public static class LocalDetection
{
    /// <summary>Steam tools that are not games.</summary>
    private static readonly string[] SteamToolPrefixes =
    [
        "Proton", "Steam Linux Runtime", "Steamworks Common Redistributables", "Steam.dll", "SteamVR",
    ];

    // ───────────── Steam ─────────────

    public static IEnumerable<string> SteamRoots(LocalEnvironment env)
    {
        if (env.IsWindows)
        {
            if (OperatingSystem.IsWindows() && WindowsRegistry.SteamPath() is { } fromRegistry)
            {
                yield return fromRegistry;
            }

            yield return @"C:\Program Files (x86)\Steam";
        }
        else
        {
            yield return Path.Combine(env.Home, ".steam", "steam");
            yield return Path.Combine(env.Home, ".local", "share", "Steam");
            yield return Path.Combine(env.Home, ".var", "app", "com.valvesoftware.Steam", "data", "Steam");
        }
    }

    /// <summary>Every Steam library folder (from libraryfolders.vdf of each Steam install).</summary>
    public static HashSet<string> SteamLibraries(LocalEnvironment env)
    {
        var libraries = new HashSet<string>(StringComparer.Ordinal);
        foreach (var root in SteamRoots(env))
        {
            var vdf = Path.Combine(root, "steamapps", "libraryfolders.vdf");
            if (!File.Exists(vdf))
            {
                continue;
            }

            libraries.Add(Canonical(root));
            if (TryRead(vdf, KeyValues.Parse) is { } parsed && parsed["libraryfolders"] is { } folders)
            {
                foreach (var folder in folders.Children.Values)
                {
                    if (folder.Get("path") is { Length: > 0 } path)
                    {
                        libraries.Add(Canonical(path));
                    }
                }
            }
        }

        return libraries;
    }

    /// <summary>DLC app ids Steam has installed for a game (InstalledDepots with a dlcappid).</summary>
    public static HashSet<string> SteamInstalledDlcs(LocalEnvironment env, string appId)
    {
        var dlcs = new HashSet<string>(StringComparer.Ordinal);
        foreach (var library in SteamLibraries(env))
        {
            var manifest = Path.Combine(library, "steamapps", $"appmanifest_{appId}.acf");
            if (!File.Exists(manifest) || TryRead(manifest, KeyValues.Parse)?["AppState"]?["InstalledDepots"] is not { } depots)
            {
                continue;
            }

            foreach (var depot in depots.Children.Values)
            {
                if (depot.Get("dlcappid") is { Length: > 0 } dlc)
                {
                    dlcs.Add(dlc);
                }
            }
        }

        return dlcs;
    }

    public static IReadOnlyList<LocalGame> DetectSteam(LocalEnvironment env)
    {
        var games = new Dictionary<string, LocalGame>();
        var libraries = SteamLibraries(env);
        foreach (var library in libraries)
        {
            var steamapps = Path.Combine(library, "steamapps");
            if (!Directory.Exists(steamapps))
            {
                continue;
            }

            foreach (var manifest in SafeFiles(steamapps, "appmanifest_*.acf"))
            {
                var app = TryRead(manifest, KeyValues.Parse)?["AppState"];
                var appId = app?.Get("appid");
                var name = app?.Get("name");
                var installDir = app?.Get("installdir");
                if (appId is null || name is null || installDir is null || games.ContainsKey(appId))
                {
                    continue;
                }

                // StateFlags bit 4 = fully installed.
                if (int.TryParse(app!.Get("StateFlags"), out var flags) && (flags & 4) == 0)
                {
                    continue;
                }

                if (SteamToolPrefixes.Any(p => name.StartsWith(p, StringComparison.OrdinalIgnoreCase)))
                {
                    continue;
                }

                games[appId] = new LocalGame(
                    "steam",
                    appId,
                    name,
                    Path.Combine(steamapps, "common", installDir),
                    null,
                    long.TryParse(app.Get("SizeOnDisk"), out var size) ? size : null,
                    "steam-manifest");
            }
        }

        return [.. games.Values.OrderBy(g => g.Title, StringComparer.OrdinalIgnoreCase)];
    }

    // ───────────── Heroic / Legendary (Linux) ─────────────

    private static IEnumerable<string> HeroicConfigDirs(LocalEnvironment env)
    {
        yield return Path.Combine(env.Home, ".config", "heroic");
        yield return Path.Combine(env.Home, ".var", "app", "com.heroicgameslauncher.hgl", "config", "heroic");
    }

    /// <summary>Heroic's GOG installs (gog_store/installed.json).</summary>
    public static IReadOnlyList<LocalGame> DetectHeroicGog(LocalEnvironment env)
    {
        var games = new List<LocalGame>();
        foreach (var dir in HeroicConfigDirs(env))
        {
            var installed = TryRead(Path.Combine(dir, "gog_store", "installed.json"), ParseJson)?["installed"]?.AsArray();
            if (installed is null)
            {
                continue;
            }

            var titles = GogLibraryTitles(Path.Combine(dir, "store_cache", "gog_library.json"));
            foreach (var entry in installed)
            {
                var appName = Str(entry, "appName");
                var path = Str(entry, "install_path");
                if (appName is null || path is null || games.Any(g => g.StoreGameId == appName))
                {
                    continue;
                }

                var title = Str(entry, "title") ?? titles.GetValueOrDefault(appName) ?? GogInfoTitle(path) ?? Path.GetFileName(path);
                games.Add(new LocalGame("gog", appName, title, path, null, null, "heroic"));
            }
        }

        return games;
    }

    private static Dictionary<string, string> GogLibraryTitles(string file)
    {
        var result = new Dictionary<string, string>(StringComparer.Ordinal);
        var games = TryRead(file, ParseJson)?["games"]?.AsArray();
        foreach (var game in games ?? [])
        {
            if (Str(game, "app_name") is { } id && Str(game, "title") is { } title)
            {
                result[id] = title;
            }
        }

        return result;
    }

    /// <summary>GOG Linux installers write the title into the first line of "gameinfo".</summary>
    private static string? GogInfoTitle(string installPath) =>
        TryRead(Path.Combine(installPath, "gameinfo"), text => text.Split('\n')[0].Trim()) is { Length: > 0 } t ? t : null;

    /// <summary>Legendary installs (Heroic's bundled copy and standalone Legendary).</summary>
    public static IReadOnlyList<LocalGame> DetectLegendary(LocalEnvironment env)
    {
        var files = HeroicConfigDirs(env)
            .Select(d => Path.Combine(d, "legendaryConfig", "legendary", "installed.json"))
            .Append(Path.Combine(env.Home, ".config", "legendary", "installed.json"));
        var games = new Dictionary<string, LocalGame>(StringComparer.Ordinal);
        foreach (var file in files)
        {
            if (TryRead(file, ParseJson) is not JsonObject installed)
            {
                continue;
            }

            foreach (var (appName, entry) in installed)
            {
                var path = Str(entry, "install_path");
                if (path is null || games.ContainsKey(appName) || entry?["is_dlc"]?.GetValue<bool>() == true)
                {
                    continue;
                }

                var exe = Str(entry, "executable");
                games[appName] = new LocalGame(
                    "epic",
                    appName,
                    Str(entry, "title") ?? appName,
                    path,
                    exe is { Length: > 0 } ? Path.Combine(path, exe) : null,
                    entry?["install_size"] is JsonValue v && v.TryGetValue(out long size) ? size : null,
                    "legendary");
            }
        }

        return [.. games.Values];
    }

    // ───────────── Amazon: Heroic (Linux), Amazon Games app (Windows) ─────────────

    /// <summary>
    /// Amazon games Heroic installed: nile's installed.json (<c>[{ id, path, size }]</c>)
    /// in Heroic's nile_config or a standalone nile config, titles from nile's library.json.
    /// </summary>
    public static IReadOnlyList<LocalGame> DetectNile(LocalEnvironment env)
    {
        var dirs = HeroicConfigDirs(env)
            .Select(d => Path.Combine(d, "nile_config", "nile"))
            .Append(Path.Combine(env.Home, ".config", "nile"));
        var games = new Dictionary<string, LocalGame>(StringComparer.Ordinal);
        foreach (var dir in dirs)
        {
            var installed = TryRead(Path.Combine(dir, "installed.json"), ParseJson) as JsonArray;
            if (installed is null)
            {
                continue;
            }

            var titles = NileLibraryTitles(Path.Combine(dir, "library.json"));
            foreach (var entry in installed)
            {
                var id = Str(entry, "id");
                var path = Str(entry, "path");
                if (id is null || path is null || games.ContainsKey(id))
                {
                    continue;
                }

                games[id] = new LocalGame(
                    "amazon",
                    id,
                    titles.GetValueOrDefault(id) ?? Path.GetFileName(path.TrimEnd('/', '\\')),
                    path,
                    null,
                    entry?["size"] is JsonValue v && v.TryGetValue(out long size) ? size : null,
                    "nile");
            }
        }

        return [.. games.Values];
    }

    private static Dictionary<string, string> NileLibraryTitles(string file)
    {
        var result = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var entitlement in (TryRead(file, ParseJson) as JsonArray) ?? [])
        {
            var product = entitlement?["product"];
            if (Str(product, "id") is { } id && Str(product, "title") is { } title)
            {
                result[id] = title;
            }
        }

        return result;
    }

    /// <summary>
    /// The product id in the uninstall command the Amazon Games app registers:
    /// <c>"…\Amazon Game Remover.exe" -m Game -p amzn1.adg.product.…</c>.
    /// </summary>
    public static string? AmazonProductId(string? uninstallCommand)
    {
        if (uninstallCommand is null || !uninstallCommand.Contains("Amazon Game Remover", StringComparison.OrdinalIgnoreCase))
        {
            return null;
        }

        var parts = uninstallCommand.Split(' ', StringSplitOptions.RemoveEmptyEntries);
        var index = Array.FindIndex(parts, p => p == "-p");
        return index >= 0 && index + 1 < parts.Length ? parts[index + 1].Trim('"') : null;
    }

    // ───────────── Epic launcher (Windows) ─────────────

    public static IReadOnlyList<LocalGame> DetectEpicManifests(LocalEnvironment env)
    {
        if (env.ProgramData is null)
        {
            return [];
        }

        var dir = Path.Combine(env.ProgramData, "Epic", "EpicGamesLauncher", "Data", "Manifests");
        var games = new List<LocalGame>();
        foreach (var file in SafeFiles(dir, "*.item"))
        {
            var item = TryRead(file, ParseJson);
            var appName = Str(item, "AppName");
            var location = Str(item, "InstallLocation");
            if (appName is null || location is null || item?["bIsIncompleteInstall"]?.GetValue<bool>() == true)
            {
                continue;
            }

            var exe = Str(item, "LaunchExecutable");
            games.Add(new LocalGame(
                "epic",
                appName,
                Str(item, "DisplayName") ?? appName,
                location,
                exe is { Length: > 0 } ? Path.Combine(location, exe) : null,
                item?["InstallSize"] is JsonValue v && v.TryGetValue(out long size) ? size : null,
                "epic-manifest"));
        }

        return games;
    }

    // ───────────── all stores ─────────────

    public static IReadOnlyList<LocalGame> DetectAll(LocalEnvironment env)
    {
        var all = new List<LocalGame>();
        all.AddRange(DetectSteam(env));
        all.AddRange(DetectHeroicGog(env));
        if (env.IsWindows && OperatingSystem.IsWindows())
        {
            all.AddRange(WindowsRegistry.GogGalaxyGames().Where(g => all.All(x => x.StoreGameId != g.StoreGameId || x.Store != "gog")));
        }

        all.AddRange(DetectLegendary(env));
        all.AddRange(DetectNile(env));
        if (env.IsWindows && OperatingSystem.IsWindows())
        {
            all.AddRange(WindowsRegistry.AmazonGames().Where(g => all.All(x => x.StoreGameId != g.StoreGameId || x.Store != "amazon")));
        }

        all.AddRange(DetectEpicManifests(env).Where(g => all.All(x => x.StoreGameId != g.StoreGameId || x.Store != "epic")));
        return all;
    }

    // ───────────── helpers ─────────────

    private static JsonNode? ParseJson(string text) => JsonNode.Parse(text);

    private static string? Str(JsonNode? node, string key) =>
        node?[key] is JsonValue v && v.TryGetValue(out string? s) && !string.IsNullOrWhiteSpace(s) ? s : null;

    private static T? TryRead<T>(string file, Func<string, T?> parse)
        where T : class
    {
        try
        {
            return File.Exists(file) ? parse(File.ReadAllText(file)) : null;
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or FormatException or JsonException or InvalidOperationException)
        {
            return null;
        }
    }

    private static IEnumerable<string> SafeFiles(string dir, string pattern)
    {
        try
        {
            return Directory.Exists(dir) ? Directory.GetFiles(dir, pattern) : [];
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException)
        {
            return [];
        }
    }

    private static string Canonical(string path)
    {
        try
        {
            var full = Path.GetFullPath(path);
            var info = new DirectoryInfo(full);
            return info.ResolveLinkTarget(returnFinalTarget: true)?.FullName ?? full.TrimEnd(Path.DirectorySeparatorChar);
        }
        catch (Exception e) when (e is IOException or UnauthorizedAccessException or ArgumentException)
        {
            return path;
        }
    }
}

/// <summary>Windows-only lookups (Steam path, GOG Galaxy and Amazon Games installs).</summary>
[SupportedOSPlatform("windows")]
internal static class WindowsRegistry
{
    public static string? SteamPath()
    {
        using var key = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(@"Software\Valve\Steam");
        return key?.GetValue("SteamPath") as string;
    }

    public static IEnumerable<LocalGame> GogGalaxyGames()
    {
        using var games = Microsoft.Win32.Registry.LocalMachine.OpenSubKey(@"SOFTWARE\WOW6432Node\GOG.com\Games");
        if (games is null)
        {
            yield break;
        }

        foreach (var id in games.GetSubKeyNames())
        {
            using var game = games.OpenSubKey(id);
            if (game?.GetValue("path") is string path && game.GetValue("gameName") is string name)
            {
                yield return new LocalGame("gog", id, name, path, game.GetValue("exe") as string, null, "gog-galaxy");
            }
        }
    }

    /// <summary>Games the Amazon Games app installed (its uninstall entries of the current user).</summary>
    public static IEnumerable<LocalGame> AmazonGames()
    {
        using var uninstall = Microsoft.Win32.Registry.CurrentUser.OpenSubKey(@"Software\Microsoft\Windows\CurrentVersion\Uninstall");
        if (uninstall is null)
        {
            yield break;
        }

        foreach (var name in uninstall.GetSubKeyNames())
        {
            using var entry = uninstall.OpenSubKey(name);
            if (LocalDetection.AmazonProductId(entry?.GetValue("UninstallString") as string) is { } id
                && entry?.GetValue("InstallLocation") is string { Length: > 0 } path
                && entry.GetValue("DisplayName") is string { Length: > 0 } title)
            {
                yield return new LocalGame("amazon", id, title, path, null, null, "amazon-games");
            }
        }
    }
}
