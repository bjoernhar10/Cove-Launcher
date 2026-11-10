using System.Text.Json.Nodes;
using Cove.Connectors.Config;
using Cove.Connectors.Http;
using Cove.Connectors.Local;
using Cove.Connectors.Model;

namespace Cove.Connectors.Connectors;

/// <summary>
/// DLCs of a Steam game: the list comes from the public store API (no login,
/// no key), which ones are installed from Steam's local app manifest. Which
/// DLCs Steam installs can only be changed in Steam itself.
/// </summary>
public sealed class SteamContent(HttpClient http, SteamConfig config)
{
    private const string Name = "Steam";

    /// <summary>At most this many DLCs are listed (some games have hundreds).</summary>
    public const int MaxDlcs = 200;

    private const int Batch = 50;

    private static string CheckApp(string appId) =>
        appId.Length is > 0 and < 12 && appId.All(char.IsAsciiDigit)
            ? appId
            : throw new ConnectorException(ConnectorErrorKind.InvalidRequest, Texts.T("Unbekanntes Steam-Spiel.", "Unknown Steam game."));

    public async Task<SteamDlcs> DlcsAsync(string appId, LocalEnvironment env, CancellationToken ct)
    {
        CheckApp(appId);
        var german = Texts.Language == "de";
        var details = await StoreHttp.GetJsonAsync(
            http,
            config.AppDetailsUrl.Replace("{appId}", appId, StringComparison.Ordinal)
                .Replace("{language}", german ? "german" : "english", StringComparison.Ordinal),
            Name, ct).ConfigureAwait(false);
        var ids = (details[appId]?["data"]?["dlc"]?.AsArray() ?? [])
            .Select(n => n?.ToString() ?? string.Empty)
            .Where(id => id.Length > 0 && id.All(char.IsAsciiDigit))
            .Distinct(StringComparer.Ordinal)
            .Take(MaxDlcs)
            .ToList();
        var names = new Dictionary<string, string>(StringComparer.Ordinal);
        foreach (var chunk in ids.Chunk(Batch))
        {
            var input = new JsonObject
            {
                ["ids"] = new JsonArray([.. chunk.Select(id => (JsonNode)new JsonObject { ["appid"] = long.Parse(id, System.Globalization.CultureInfo.InvariantCulture) })]),
                ["context"] = new JsonObject { ["language"] = german ? "german" : "english", ["country_code"] = german ? "DE" : "US" },
                ["data_request"] = new JsonObject(),
            };
            try
            {
                var items = await StoreHttp.GetJsonAsync(
                    http,
                    config.StoreItemsUrl.Replace("{input}", Uri.EscapeDataString(input.ToJsonString()), StringComparison.Ordinal),
                    Name, ct).ConfigureAwait(false);
                foreach (var item in items["response"]?["store_items"]?.AsArray() ?? [])
                {
                    if (StoreHttp.Optional(item, "appid") is { } id && StoreHttp.Optional(item, "name") is { Length: > 0 } name)
                    {
                        names[id] = name;
                    }
                }
            }
            catch (ConnectorException)
            {
                // Names are a nicety: without them the list shows the app ids.
            }
        }

        var installed = LocalDetection.SteamInstalledDlcs(env, appId);
        var dlcs = ids
            .Select(id => new SteamDlc(id, names.GetValueOrDefault(id) ?? $"DLC {id}", installed.Contains(id)))
            .OrderByDescending(d => d.Installed)
            .ThenBy(d => d.Title, StringComparer.CurrentCultureIgnoreCase)
            .ToList();
        return new SteamDlcs(dlcs, details[appId]?["data"]?["dlc"]?.AsArray().Count ?? 0);
    }
}
