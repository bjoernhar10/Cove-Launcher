using System.Text.Json;
using System.Text.Json.Serialization;

namespace Cove.Connectors.Config;

public sealed record SteamConfig(string OpenIdEndpoint, string PrivacySettingsUrl);

public sealed record GogConfig(
    string ClientId,
    string ClientSecret,
    string AuthUrl,
    string TokenUrl,
    string RedirectUri,
    string UserDataUrl,
    string OwnedProductsUrl);

public sealed record EpicConfig(
    string ClientId,
    string ClientSecret,
    string LoginUrl,
    string RedirectUrl,
    string TokenUrl,
    string LibraryUrl,
    string PlaytimeUrl,
    string CatalogUrl);

/// <summary>Store endpoints and client ids (Config/connectors.json, embedded).</summary>
public sealed record ConnectorConfig(SteamConfig Steam, GogConfig Gog, EpicConfig Epic)
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
