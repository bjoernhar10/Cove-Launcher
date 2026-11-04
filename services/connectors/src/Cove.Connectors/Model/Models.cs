using System.Text.Json.Serialization;

namespace Cove.Connectors.Model;

// Wire types; mirror packages/shared/src/models.ts (Connector* types).

/// <summary>How the core shows the login page.</summary>
[JsonConverter(typeof(JsonStringEnumConverter<LoginMode>))]
public enum LoginMode
{
    /// <summary>System browser with a loopback redirect (RFC 8252).</summary>
    [JsonStringEnumMemberName("browser")]
    Browser,

    /// <summary>Own Tauri webview; the core intercepts navigation to <see cref="LoginStart.CallbackPrefix"/>.</summary>
    [JsonStringEnumMemberName("webview")]
    Webview,
}

public sealed record LoginStart(
    string Store,
    string Url,
    LoginMode Mode,
    string? CallbackPrefix,
    string? FallbackUrl);

/// <summary>What the core stores in the OS keyring after a login. Never logged.</summary>
public sealed record StoreSession(
    string Store,
    string AccountId,
    string DisplayName,
    string? AccessToken,
    string? RefreshToken,
    DateTimeOffset? ExpiresAt);

public sealed record OwnedGame(
    string StoreGameId,
    string Title,
    long? PlaytimeMinutes,
    DateTimeOffset? LastPlayedAt,
    string? ImageUrl);

public sealed record StoreAchievement(
    string Id,
    string Title,
    string Description,
    string? IconUrl,
    string? IconLockedUrl,
    bool Hidden,
    DateTimeOffset? UnlockedAt,
    double? RarityPercent);

public sealed record LocalGame(
    string Store,
    string StoreGameId,
    string Title,
    string InstallPath,
    string? ExecutablePath,
    long? SizeBytes,
    string Source);

/// <summary>Store-side failure with a stable kind the core maps to UI states.</summary>
public sealed class ConnectorException(ConnectorErrorKind kind, string message, Exception? inner = null)
    : Exception(message, inner)
{
    public ConnectorErrorKind Kind { get; } = kind;

    /// <summary>Page where the user can fix the problem (e.g. Steam privacy settings).</summary>
    public string? HelpUrl { get; init; }
}

[JsonConverter(typeof(JsonStringEnumConverter<ConnectorErrorKind>))]
public enum ConnectorErrorKind
{
    /// <summary>Login/refresh rejected: the user must sign in again.</summary>
    AuthFailed,

    /// <summary>Steam profile or game details are private.</summary>
    PrivateProfile,

    /// <summary>The store answered unexpectedly (API changed, outage).</summary>
    Unavailable,

    /// <summary>No connection.</summary>
    Network,

    /// <summary>Bad input from the core.</summary>
    InvalidRequest,
}
