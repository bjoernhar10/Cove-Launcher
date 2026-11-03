using System.Text.Json.Nodes;
using Cove.Connectors.Rpc;
using Xunit;

namespace Cove.Connectors.Tests;

public class RpcDispatcherTests
{
    private static JsonObject Handle(string line) =>
        JsonNode.Parse(RpcDispatcher.HandleLine(line) ?? throw new InvalidOperationException("no response"))!
            .AsObject();

    [Fact]
    public void PingReturnsPong()
    {
        var response = Handle("""{"jsonrpc":"2.0","id":1,"method":"system.ping","params":{"message":"hi"}}""");

        Assert.Equal(1, response["id"]!.GetValue<int>());
        Assert.Equal("connectors", response["result"]!["service"]!.GetValue<string>());
        Assert.Equal("pong: hi", response["result"]!["message"]!.GetValue<string>());
    }

    [Fact]
    public void UnknownMethodReturnsMethodNotFound()
    {
        var response = Handle("""{"jsonrpc":"2.0","id":2,"method":"nope"}""");
        Assert.Equal(RpcDispatcher.MethodNotFound, response["error"]!["code"]!.GetValue<int>());
    }

    [Fact]
    public void InvalidJsonReturnsParseError()
    {
        var response = Handle("{not json");
        Assert.Equal(RpcDispatcher.ParseError, response["error"]!["code"]!.GetValue<int>());
    }

    [Fact]
    public void NotificationHasNoResponse()
    {
        Assert.Null(RpcDispatcher.HandleLine("""{"jsonrpc":"2.0","method":"system.ping"}"""));
    }
}
