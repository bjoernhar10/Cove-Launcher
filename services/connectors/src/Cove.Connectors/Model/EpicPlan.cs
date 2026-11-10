namespace Cove.Connectors.Model;

// Epic installs: the connector does everything that needs the session; the
// core downloads and parses manifests and chunks (apps/desktop/src-tauri/src/downloads/epic.rs).

/// <summary>Where a game's manifest lives and what the catalogue says about it.</summary>
/// <param name="ManifestUrls">Signed manifest links on Epic's CDNs, best first.</param>
/// <param name="LauncherOnly">Why the game must stay with the Epic Launcher (third-party launcher), or null.</param>
/// <param name="CloudSaveFolder">Save folder with variables like {AppData}, or null without cloud saves.</param>
/// <param name="Dlcs">Owned DLCs with files of their own on this platform.</param>
public sealed record EpicInstallInfo(
    string AppName,
    string Namespace,
    string CatalogItemId,
    string BuildVersion,
    IReadOnlyList<string> ManifestUrls,
    string? LauncherOnly,
    string? CloudSaveFolder,
    bool OwnershipToken,
    IReadOnlyList<EpicDlc> Dlcs);

/// <summary>A DLC with its own build: installed into the game's folder, updated with it.</summary>
public sealed record EpicDlc(
    string AppName,
    string CatalogItemId,
    string Title,
    string BuildVersion,
    IReadOnlyList<string> ManifestUrls);

/// <summary>Everything a start needs: a one-time exchange code and, for some games, an ownership token.</summary>
public sealed record EpicLaunch(
    string ExchangeCode,
    string AccountId,
    string DisplayName,
    string? OwnershipToken);

/// <summary>One file of the cloud saves (manifests and chunks) with a signed read link.</summary>
public sealed record EpicSaveFile(string Path, string? ReadLink, DateTimeOffset? LastModified, long? Size);

public sealed record EpicSaves(IReadOnlyList<EpicSaveFile> Files);

public sealed record EpicWriteLinks(Dictionary<string, string> Links);
