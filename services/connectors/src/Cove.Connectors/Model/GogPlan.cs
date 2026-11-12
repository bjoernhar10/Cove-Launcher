namespace Cove.Connectors.Model;

// GOG install plans: built here from GOG's content system, downloaded by the
// core's download manager. Mirror: apps/desktop/src-tauri/src/downloads/gog.rs.

/// <summary>One zlib-compressed piece of a file on GOG's CDN.</summary>
public sealed record GogChunk(string Md5, string CompressedMd5, long Size, long CompressedSize);

/// <summary>A file stored inside a small-files container (index, offset, size).</summary>
public sealed record GogSfcRef(int Container, long Offset, long Size);

/// <summary>A file of the game; either its own chunks or a slice of a container.</summary>
public sealed record GogFile(string Path, string ProductId, IReadOnlyList<GogChunk> Chunks, bool Executable, GogSfcRef? Sfc);

/// <summary>Many small files packed into one download.</summary>
public sealed record GogContainer(string ProductId, IReadOnlyList<GogChunk> Chunks);

public sealed record GogLink(string Path, string Target);

/// <summary>Native Linux games come as one installer (a shell script with a zip inside).</summary>
public sealed record GogInstaller(string Url, string FileName, long Size);

/// <summary>An owned DLC whose depots are part of the plan (files carry its product id).</summary>
public sealed record GogDlc(string ProductId, string Title);

/// <summary>Everything the core needs to install or update one GOG game.</summary>
/// <param name="Platform">windows or linux: what the files are built for.</param>
/// <param name="ChunkUrls">Per product: CDN URL templates, best first; <c>{chunk}</c> = "ab/cd/abcd…".</param>
public sealed record GogInstallPlan(
    string ProductId,
    string Platform,
    string BuildId,
    string Version,
    long DownloadBytes,
    long InstallBytes,
    IReadOnlyList<string> Directories,
    IReadOnlyList<GogFile> Files,
    IReadOnlyList<GogLink> Links,
    IReadOnlyList<GogContainer> Containers,
    Dictionary<string, List<string>> ChunkUrls,
    GogInstaller? Installer,
    IReadOnlyList<GogDlc> Dlcs);

/// <summary>A folder Galaxy syncs: its name in the cloud and where it lives (<c>&lt;?DOCUMENTS?&gt;/…</c>).</summary>
public sealed record GogSaveLocation(string Name, string Location);

/// <summary>
/// Cloud saves of a game: the storage URL and a token for the game's own Galaxy
/// client. The core lists, downloads and uploads the files itself.
/// </summary>
public sealed record GogCloudSaves(
    bool Enabled,
    string? ClientId,
    string? UserId,
    string? AccessToken,
    string? StorageUrl,
    IReadOnlyList<GogSaveLocation> Locations)
{
    public static GogCloudSaves None { get; } = new(false, null, null, null, null, []);
}

/// <summary>The newest public build (for update checks).</summary>
public sealed record GogLatest(string BuildId, string Version, string Platform);
