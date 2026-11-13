using System.Text.Json;
using System.Text.Json.Serialization;

namespace Cove.Connectors.Config;

/// <param name="CoverUrl">Portrait library artwork; <c>{appId}</c> is replaced. The game icon is the fallback.</param>
public sealed record SteamConfig(
    string OpenIdEndpoint,
    string PrivacySettingsUrl,
    string? CoverUrl = null,
    string AppDetailsUrl = "https://store.steampowered.com/api/appdetails?appids={appId}&filters=basic&l={language}",
    string StoreItemsUrl = "https://api.steampowered.com/IStoreBrowseService/GetItems/v1/?input_json={input}");

public sealed record GogConfig(
    string ClientId,
    string ClientSecret,
    string AuthUrl,
    string TokenUrl,
    string RedirectUri,
    string UserDataUrl,
    string OwnedProductsUrl,
    string OwnedIdsUrl,
    string BuildsUrl,
    string MetaUrl,
    string SecureLinkUrl,
    string ProductDownloadsUrl,
    string ProductDlcsUrl,
    string RemoteConfigUrl,
    string CloudStorageUrl);

public sealed record EpicConfig(
    string ClientId,
    string ClientSecret,
    string LoginUrl,
    string RedirectUrl,
    string TokenUrl,
    string LibraryUrl,
    string PlaytimeUrl,
    string CatalogUrl,
    string AssetsUrl,
    string ExchangeUrl,
    string OwnershipTokenUrl,
    string SavesUrl);

/// <param name="DeviceType">Device type of the Amazon Games app (part of the device client id).</param>
/// <param name="LauncherAgent">Sent as the <c>UserAgent</c> header of distribution calls, like the app does.</param>
public sealed record AmazonConfig(
    string SigninUrl,
    string ReturnTo,
    string MarketplaceId,
    string DeviceType,
    string AppName,
    string AppVersion,
    string UserAgent,
    string LauncherAgent,
    string KeyId,
    string RegisterUrl,
    string TokenUrl,
    string EntitlementsUrl,
    string DistributionUrl,
    string SdkChannelUrl);

/// <summary>Store endpoints and client ids (Config/connectors.json, embedded).</summary>
public sealed record ConnectorConfig(SteamConfig Steam, GogConfig Gog, EpicConfig Epic, AmazonConfig Amazon)
{
    /// <summary>Embedded defaults, or the file named by COVE_CONNECTORS_CONFIG.</summary>
    public static ConnectorConfig Load()
    {
        var overridePath = Environment.GetEnvironmentVariable("COVE_CONNECTORS_CONFIG");
        if (!string.IsNullOrEmpty(overridePath) && File.Exists(overridePath))
        {
            return Parse(File.ReadAllText(overridePath));
        }

        using var stream = typeof(ConnectorConfig).Assembly.GetManifestResourceStream("Cove.Connectors.Config.connectors.json")
            ?? throw new InvalidOperationException("connectors.json is not embedded");
        using var reader = new StreamReader(stream);
        return Parse(reader.ReadToEnd());
    }

    public static ConnectorConfig Parse(string json) =>
        JsonSerializer.Deserialize(json, ConfigJsonContext.Default.ConnectorConfig)
            ?? throw new InvalidOperationException("invalid connectors.json");
}

[JsonSourceGenerationOptions(PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase)]
[JsonSerializable(typeof(ConnectorConfig))]
internal sealed partial class ConfigJsonContext : JsonSerializerContext
{
}
