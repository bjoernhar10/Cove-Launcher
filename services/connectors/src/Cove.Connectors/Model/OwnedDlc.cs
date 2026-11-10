namespace Cove.Connectors.Model;

/// <summary>A DLC the user owns for a GOG or Epic game.</summary>
/// <param name="Id">GOG product id or Epic app name.</param>
/// <param name="UnlockOnly">True when it has no files of its own: the game unlocks it through the login.</param>
public sealed record OwnedDlc(string Id, string Title, bool UnlockOnly);

public sealed record OwnedDlcs(IReadOnlyList<OwnedDlc> Dlcs);
