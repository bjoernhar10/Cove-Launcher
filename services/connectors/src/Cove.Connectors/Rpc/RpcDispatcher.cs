using System.Text.Json;
using System.Text.Json.Nodes;

namespace Cove.Connectors.Rpc;

/// <summary>Minimal JSON-RPC 2.0 dispatcher. One JSON message per line (NDJSON over stdio).</summary>
public static class RpcDispatcher
{
    public const string Version = "0.1.0";

    public const int ParseError = -32700;
    public const int InvalidRequest = -32600;
    public const int MethodNotFound = -32601;
    public const int InvalidParams = -32602;

    /// <summary>Handles one raw input line. Returns the serialized response, or null for notifications.</summary>
    public static string? HandleLine(string line)
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

        JsonNode? result;
        try
        {
            result = method switch
            {
                "system.ping" => Ping(msg["params"]),
                _ => null,
            };
        }
        catch (JsonException ex)
        {
            return isNotification ? null : Error(id, InvalidParams, ex.Message);
        }

        if (isNotification)
        {
            return null;
        }

        return result is null
            ? Error(id, MethodNotFound, "Method not found")
            : new JsonObject { ["jsonrpc"] = "2.0", ["id"] = id, ["result"] = result }.ToJsonString();
    }

    private static JsonNode? Ping(JsonNode? paramsNode)
    {
        var p = paramsNode is null
            ? new PingParams(null)
            : JsonSerializer.Deserialize(paramsNode, RpcJsonContext.Default.PingParams) ?? new PingParams(null);

        var result = new PingResult("connectors", Version, $"pong: {p.Message}");
        return JsonSerializer.SerializeToNode(result, RpcJsonContext.Default.PingResult);
    }

    private static string? AsString(JsonNode? node) =>
        node is JsonValue value && value.TryGetValue(out string? s) ? s : null;

    private static string Error(JsonNode? id, int code, string message) =>
        new JsonObject
        {
            ["jsonrpc"] = "2.0",
            ["id"] = id,
            ["error"] = new JsonObject { ["code"] = code, ["message"] = message },
        }.ToJsonString();
}
