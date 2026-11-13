using System.Text.Json.Serialization;
using Cove.Connectors.Model;

namespace Cove.Connectors.Rpc;

// Shapes mirror packages/shared — keep them in sync.

public sealed record PingParams(string? Message);

public sealed record PingResult(string Service, string Version, string Message);

public sealed record SetLanguageParams(string Language);

public sealed record SetLanguageResult(string Language);

public sealed record StoreInfo(string Id, string Name);

public sealed record LoginStartParams(string Store, string RedirectUri, string State);

public sealed record LoginCompleteParams(string Store, string? CallbackUrl, string? Code, string? ExpectedReturnTo, string? ProxyUrl);

public sealed record SessionParams(StoreSession Session, string? ProxyUrl);

public sealed record AchievementsParams(StoreSession Session, string StoreGameId, string? ProxyUrl);

public sealed record DetectLocalParams(string Home, string? ProgramData, string? Store);

public sealed record SteamDlcParams(string AppId, string Home, string? ProgramData);

/// <param name="Platform">windows, linux or osx: the system Cove runs on.</param>
/// <param name="Language">de or en: preferred game language.</param>
public sealed record GogPlanParams(StoreSession Session, string ProductId, string Platform, string Language, string? ProxyUrl);

public sealed record GogDlcParams(StoreSession Session, string ProductId, string Platform, string? ProxyUrl);

public sealed record EpicAppParams(StoreSession Session, string AppName, string? Platform, bool OwnershipToken, IReadOnlyList<string>? Paths, string? ProxyUrl);

public sealed record AmazonParams(StoreSession Session, string? ProductId, IReadOnlyList<string>? ProductIds, string? ProxyUrl);

public sealed record LibraryResult(IReadOnlyList<OwnedGame> Games);

public sealed record AchievementsResult(IReadOnlyList<StoreAchievement> Achievements);

public sealed record LocalResult(IReadOnlyList<LocalGame> Games);

/// <summary>Source-generated JSON metadata (required for Native AOT — no reflection).</summary>
[JsonSourceGenerationOptions(
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    DefaultIgnoreCondition = JsonIgnoreCondition.Never)]
[JsonSerializable(typeof(PingParams))]
[JsonSerializable(typeof(PingResult))]
[JsonSerializable(typeof(SetLanguageParams))]
[JsonSerializable(typeof(SetLanguageResult))]
[JsonSerializable(typeof(List<StoreInfo>))]
[JsonSerializable(typeof(LoginStartParams))]
[JsonSerializable(typeof(LoginStart))]
[JsonSerializable(typeof(LoginCompleteParams))]
[JsonSerializable(typeof(StoreSession))]
[JsonSerializable(typeof(SessionParams))]
[JsonSerializable(typeof(AchievementsParams))]
[JsonSerializable(typeof(DetectLocalParams))]
[JsonSerializable(typeof(SteamDlcParams))]
[JsonSerializable(typeof(SteamDlcs))]
[JsonSerializable(typeof(LibraryResult))]
[JsonSerializable(typeof(AchievementsResult))]
[JsonSerializable(typeof(LocalResult))]
[JsonSerializable(typeof(GogPlanParams))]
[JsonSerializable(typeof(GogDlcParams))]
[JsonSerializable(typeof(GogCloudSaves))]
[JsonSerializable(typeof(OwnedDlcs))]
[JsonSerializable(typeof(GogInstallPlan))]
[JsonSerializable(typeof(GogLatest))]
[JsonSerializable(typeof(EpicAppParams))]
[JsonSerializable(typeof(EpicInstallInfo))]
[JsonSerializable(typeof(EpicLaunch))]
[JsonSerializable(typeof(EpicSaves))]
[JsonSerializable(typeof(EpicWriteLinks))]
[JsonSerializable(typeof(AmazonParams))]
[JsonSerializable(typeof(AmazonDownload))]
[JsonSerializable(typeof(AmazonVersions))]
[JsonSerializable(typeof(AmazonSdk))]
internal sealed partial class RpcJsonContext : JsonSerializerContext
{
}
