using System.Text.Json.Serialization;
using Cove.Connectors.Model;

namespace Cove.Connectors.Rpc;

// Shapes mirror packages/shared — keep them in sync.

public sealed record PingParams(string? Message);

public sealed record PingResult(string Service, string Version, string Message);

public sealed record StoreInfo(string Id, string Name);

public sealed record LoginStartParams(string Store, string RedirectUri, string State);

public sealed record LoginCompleteParams(string Store, string? CallbackUrl, string? Code, string? ExpectedReturnTo, string? ProxyUrl);

public sealed record SessionParams(StoreSession Session, string? ProxyUrl);

public sealed record AchievementsParams(StoreSession Session, string StoreGameId, string? ProxyUrl);

public sealed record DetectLocalParams(string Home, string? ProgramData, string? Store);

public sealed record LibraryResult(IReadOnlyList<OwnedGame> Games);

public sealed record AchievementsResult(IReadOnlyList<StoreAchievement> Achievements);

public sealed record LocalResult(IReadOnlyList<LocalGame> Games);

/// <summary>Source-generated JSON metadata (required for Native AOT — no reflection).</summary>
[JsonSourceGenerationOptions(
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    DefaultIgnoreCondition = JsonIgnoreCondition.Never)]
[JsonSerializable(typeof(PingParams))]
[JsonSerializable(typeof(PingResult))]
[JsonSerializable(typeof(List<StoreInfo>))]
[JsonSerializable(typeof(LoginStartParams))]
[JsonSerializable(typeof(LoginStart))]
[JsonSerializable(typeof(LoginCompleteParams))]
[JsonSerializable(typeof(StoreSession))]
[JsonSerializable(typeof(SessionParams))]
[JsonSerializable(typeof(AchievementsParams))]
[JsonSerializable(typeof(DetectLocalParams))]
[JsonSerializable(typeof(LibraryResult))]
[JsonSerializable(typeof(AchievementsResult))]
[JsonSerializable(typeof(LocalResult))]
internal sealed partial class RpcJsonContext : JsonSerializerContext
{
}
