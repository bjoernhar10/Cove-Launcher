namespace Cove.Connectors.Model;

/// <summary>A DLC of a Steam game and whether Steam has it installed.</summary>
public sealed record SteamDlc(string AppId, string Title, bool Installed);

/// <param name="Total">All DLCs of the game (the list stops at <c>SteamContent.MaxDlcs</c>).</param>
public sealed record SteamDlcs(IReadOnlyList<SteamDlc> Dlcs, int Total);
