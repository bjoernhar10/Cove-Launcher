using Cove.Connectors.Model;

namespace Cove.Connectors.Connectors;

public sealed record LoginCompleteRequest(string? CallbackUrl, string? Code, string? ExpectedReturnTo);

/// <summary>Per-call context from the core (e.g. the catalogue proxy for Steam data).</summary>
public sealed record StoreContext(string? ProxyUrl);

/// <summary>Where to look for locally installed games.</summary>
public sealed record LocalEnvironment(string Home, string? ProgramData, bool IsWindows);

/// <summary>One store. Tokens come from the core with every call; nothing is stored here.</summary>
public interface IStoreConnector
{
    string Id { get; }

    string Name { get; }

    LoginStart LoginStart(string redirectUri, string state);

    Task<StoreSession> LoginCompleteAsync(LoginCompleteRequest request, StoreContext context, CancellationToken ct);

    Task<StoreSession> RefreshAsync(StoreSession session, CancellationToken ct);

    Task<IReadOnlyList<OwnedGame>> GetLibraryAsync(StoreSession session, StoreContext context, CancellationToken ct);

    Task<IReadOnlyList<StoreAchievement>> GetAchievementsAsync(StoreSession session, string storeGameId, StoreContext context, CancellationToken ct);

    IReadOnlyList<LocalGame> DetectLocal(LocalEnvironment environment);
}
