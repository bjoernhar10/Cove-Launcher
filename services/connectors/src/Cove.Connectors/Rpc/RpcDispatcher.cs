using System.Text.Json;
using System.Text.Json.Nodes;
using System.Text.Json.Serialization.Metadata;
using Cove.Connectors.Config;
using Cove.Connectors.Connectors;
using Cove.Connectors.Model;

namespace Cove.Connectors.Rpc;

/// <summary>JSON-RPC 2.0 dispatcher. One JSON message per line (NDJSON over stdio).</summary>
public sealed class RpcDispatcher
{
    public const string Version = "0.1.0";

    public const int ParseError = -32700;
    public const int InvalidRequest = -32600;
    public const int MethodNotFound = -32601;
    public const int InvalidParams = -32602;

    /// <summary>Store failure; <c>error.data.kind</c> says which (authFailed, privateProfile, …).</summary>
    public const int StoreError = 2000;

    private readonly Dictionary<string, IStoreConnector> _connectors;

    public RpcDispatcher(IEnumerable<IStoreConnector> connectors)
    {
        _connectors = connectors.ToDictionary(c => c.Id, StringComparer.Ordinal);
    }

    /// <summary>Production wiring: shared HttpClient and the embedded config.</summary>
    public static RpcDispatcher CreateDefault(HttpClient http)
    {
        var config = ConnectorConfig.Load();
        return new RpcDispatcher([
            new SteamConnector(http, config.Steam),
            new GogConnector(http, config.Gog),
            new EpicConnector(http, config.Epic),
        ]);
    }

    /// <summary>Handles one raw input line. Returns the serialized response, or null for notifications.</summary>
    public async Task<string?> HandleLineAsync(string line, CancellationToken ct = default)
    {
        JsonNode? node;
        try
        {
            node = JsonNode.Parse(line);
        }
        catch (JsonException)
        {
            return Error(null, ParseError, "Parse error");
        }

        if (node is not JsonObject msg
            || AsString(msg["jsonrpc"]) != "2.0"
            || AsString(msg["method"]) is not { } method)
        {
            return Error(null, InvalidRequest, "Invalid Request");
        }

        var isNotification = !msg.ContainsKey("id");
        var id = msg["id"]?.DeepClone();
        var paramsNode = msg["params"];

        JsonNode? result;
        try
        {
            result = await DispatchAsync(method, paramsNode, ct).ConfigureAwait(false);
        }
        catch (ConnectorException e)
        {
            return isNotification ? null : Error(id, StoreError, e.Message, KindName(e.Kind), e.HelpUrl);
        }
        catch (JsonException e)
        {
            return isNotification ? null : Error(id, InvalidParams, $"Invalid params: {e.Message}");
        }
        catch (Exception e) when (e is InvalidOperationException or FormatException or KeyNotFoundException)
        {
            // A store answered in an unexpected shape (undocumented APIs change): report, never crash.
            return isNotification ? null : Error(id, StoreError, Texts.T("Der Store hat unerwartet geantwortet. Bitte später erneut versuchen.", "The store answered unexpectedly. Please try again later."), "unavailable");
        }

        if (isNotification)
        {
            return null;
        }

        return result is null
            ? Error(id, MethodNotFound, "Method not found")
            : new JsonObject { ["jsonrpc"] = "2.0", ["id"] = id, ["result"] = result }.ToJsonString();
    }

    private static T Params<T>(JsonNode? node, JsonTypeInfo<T> type) =>
        (node is null ? default : JsonSerializer.Deserialize(node, type)) ?? throw new JsonException("params missing");

    private IStoreConnector Connector(string store) =>
        _connectors.GetValueOrDefault(store)
            ?? throw new ConnectorException(ConnectorErrorKind.InvalidRequest, Texts.T($"Unbekannter Store: {store}", $"Unknown store: {store}"));

    private static JsonNode? Node<T>(T value, JsonTypeInfo<T> type) => JsonSerializer.SerializeToNode(value, type);

    private async Task<JsonNode?> DispatchAsync(string method, JsonNode? p, CancellationToken ct)
    {
        var ctx = RpcJsonContext.Default;
        switch (method)
        {
            case "system.ping":
                {
                    var ping = p is null ? new PingParams(null) : Params(p, ctx.PingParams);
                    return Node(new PingResult("connectors", Version, $"pong: {ping.Message}"), ctx.PingResult);
                }

            case "system.setLanguage":
                {
                    var args = Params(p, ctx.SetLanguageParams);
                    if (!Texts.SetLanguage(args.Language))
                    {
                        throw new JsonException("language must be 'de' or 'en'");
                    }

                    return Node(new SetLanguageResult(Texts.Language), ctx.SetLanguageResult);
                }

            case "store.list":
                return Node(_connectors.Values.Select(c => new StoreInfo(c.Id, c.Name)).ToList(), ctx.ListStoreInfo);

            case "store.loginStart":
                {
                    var args = Params(p, ctx.LoginStartParams);
                    return Node(Connector(args.Store).LoginStart(args.RedirectUri, args.State), ctx.LoginStart);
                }

            case "store.loginComplete":
                {
                    var args = Params(p, ctx.LoginCompleteParams);
                    var session = await Connector(args.Store)
                        .LoginCompleteAsync(new LoginCompleteRequest(args.CallbackUrl, args.Code, args.ExpectedReturnTo), new StoreContext(args.ProxyUrl), ct)
                        .ConfigureAwait(false);
                    return Node(session, ctx.StoreSession);
                }

            case "store.refresh":
                {
                    var args = Params(p, ctx.SessionParams);
                    var session = await Connector(args.Session.Store).RefreshAsync(args.Session, ct).ConfigureAwait(false);
                    return Node(session, ctx.StoreSession);
                }

            case "store.library":
                {
                    var args = Params(p, ctx.SessionParams);
                    var games = await Connector(args.Session.Store)
                        .GetLibraryAsync(args.Session, new StoreContext(args.ProxyUrl), ct).ConfigureAwait(false);
                    return Node(new LibraryResult(games), ctx.LibraryResult);
                }

            case "store.achievements":
                {
                    var args = Params(p, ctx.AchievementsParams);
                    var list = await Connector(args.Session.Store)
                        .GetAchievementsAsync(args.Session, args.StoreGameId, new StoreContext(args.ProxyUrl), ct).ConfigureAwait(false);
                    return Node(new AchievementsResult(list), ctx.AchievementsResult);
                }

            case "store.detectLocal":
                {
                    var args = Params(p, ctx.DetectLocalParams);
                    var env = new LocalEnvironment(args.Home, args.ProgramData, OperatingSystem.IsWindows());
                    var games = args.Store is null
                        ? Local.LocalDetection.DetectAll(env)
                        : Connector(args.Store).DetectLocal(env);
                    return Node(new LocalResult(games), ctx.LocalResult);
                }

            default:
                return null;
        }
    }

    private static string KindName(ConnectorErrorKind kind) => kind switch
    {
        ConnectorErrorKind.AuthFailed => "authFailed",
        ConnectorErrorKind.PrivateProfile => "privateProfile",
        ConnectorErrorKind.Unavailable => "unavailable",
        ConnectorErrorKind.Network => "network",
        _ => "invalidRequest",
    };

    private static string? AsString(JsonNode? node) =>
        node is JsonValue value && value.TryGetValue(out string? s) ? s : null;

    private static string Error(JsonNode? id, int code, string message, string? kind = null, string? helpUrl = null)
    {
        var error = new JsonObject { ["code"] = code, ["message"] = message };
        if (kind is not null)
        {
            error["data"] = new JsonObject { ["kind"] = kind, ["helpUrl"] = helpUrl };
        }

        return new JsonObject { ["jsonrpc"] = "2.0", ["id"] = id, ["error"] = error }.ToJsonString();
    }
}
