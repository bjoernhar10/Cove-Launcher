using System.Text.Json.Nodes;
using Cove.Connectors.Http;
using Cove.Connectors.Model;

namespace Cove.Connectors.Connectors;

/// <summary>
/// What an Amazon install needs from the session: the download location of a
/// game's current version, the live versions (update check) and the location of
/// the Amazon Games SDK files some games load. The core downloads and parses the
/// manifests (apps/desktop/src-tauri/src/downloads/amazon.rs).
/// </summary>
public sealed class AmazonContent(HttpClient http, AmazonConnector connector, Config.AmazonConfig config)
{
    private const string Store = "Amazon";

    public async Task<AmazonDownload> DownloadAsync(StoreSession session, string productId, CancellationToken ct)
    {
        var entitlements = await connector.EntitlementsAsync(session, ct).ConfigureAwait(false);
        var entitlement = entitlements.FirstOrDefault(e => StoreHttp.Optional(e["product"], "id") == productId)
            ?? throw new ConnectorException(ConnectorErrorKind.InvalidRequest, Texts.T("Dieses Spiel ist nicht in deiner Amazon-Bibliothek.", "This game isn't in your Amazon library."));
        var entitlementId = StoreHttp.Required(entitlement, "id", Store);
        using var request = connector.Distribution(
            config.DistributionUrl,
            "com.amazon.animusdistributionservice.external.AnimusDistributionService.GetGameDownload",
            session.AccessToken ?? "",
            new JsonObject { ["entitlementId"] = entitlementId, ["Operation"] = "GetGameDownload" });
        var answer = await StoreHttp.SendJsonAsync(http, request, Store, ct).ConfigureAwait(false);
        return new AmazonDownload(
            productId,
            entitlementId,
            StoreHttp.Optional(entitlement["product"], "sku") ?? "",
            StoreHttp.Required(answer, "versionId", Store),
            StoreHttp.Required(answer, "downloadUrl", Store));
    }

    /// <summary>The live version of each product.</summary>
    public async Task<AmazonVersions> VersionsAsync(StoreSession session, IReadOnlyList<string> productIds, CancellationToken ct)
    {
        using var request = connector.Distribution(
            config.DistributionUrl,
            "com.amazon.animusdistributionservice.external.AnimusDistributionService.GetLiveVersionIds",
            session.AccessToken ?? "",
            new JsonObject { ["adgProductIds"] = new JsonArray([.. productIds.Select(id => JsonValue.Create(id))]), ["Operation"] = "GetLiveVersions" });
        var answer = await StoreHttp.SendJsonAsync(http, request, Store, ct).ConfigureAwait(false);
        var map = new Dictionary<string, string>(StringComparer.Ordinal);
        if (answer["adgProductIdToVersionIdMap"] is JsonObject versions)
        {
            foreach (var (id, version) in versions)
            {
                if (version is JsonValue v && v.TryGetValue(out string? s))
                {
                    map[id] = s;
                }
            }
        }

        return new AmazonVersions(map);
    }

    /// <summary>Where the Amazon Games SDK files are (FuelSDK, AmazonGamesSDK).</summary>
    public async Task<AmazonSdk> SdkAsync(StoreSession session, CancellationToken ct)
    {
        using var request = new HttpRequestMessage(HttpMethod.Get, config.SdkChannelUrl);
        request.Headers.TryAddWithoutValidation("x-amzn-token", session.AccessToken ?? "");
        var answer = await StoreHttp.SendJsonAsync(http, request, Store, ct).ConfigureAwait(false);
        return new AmazonSdk(StoreHttp.Required(answer, "versionId", Store), StoreHttp.Required(answer, "downloadUrl", Store));
    }
}
