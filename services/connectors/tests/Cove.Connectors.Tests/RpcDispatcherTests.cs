using System.Text.Json.Nodes;
using Cove.Connectors.Rpc;
using Xunit;

namespace Cove.Connectors.Tests;

[Collection("Language")]
public class RpcDispatcherTests
{
    private static readonly RpcDispatcher Dispatcher = RpcDispatcher.CreateDefault(
        new HttpClient(new StubHandler((_, _) => throw new HttpRequestException("offline in tests"))));

    private static async Task<JsonObject> Handle(string line) =>
        JsonNode.Parse(await Dispatcher.HandleLineAsync(line, TestContext.Current.CancellationToken) ?? throw new InvalidOperationException("no response"))!.AsObject();

    [Fact]
    public async Task PingAndStoreList()
    {
        var ping = await Handle("""{"jsonrpc":"2.0","id":1,"method":"system.ping","params":{"message":"hi"}}""");
        Assert.Equal("connectors", ping["result"]!["service"]!.GetValue<string>());
        var list = await Handle("""{"jsonrpc":"2.0","id":2,"method":"store.list"}""");
        Assert.Equal(["steam", "gog", "epic"], list["result"]!.AsArray().Select(s => s!["id"]!.GetValue<string>()));
    }

    [Fact]
    public async Task SetLanguageSwitchesErrorMessages()
    {
        try
        {
            var set = await Handle("""{"jsonrpc":"2.0","id":7,"method":"system.setLanguage","params":{"language":"en"}}""");
            Assert.Equal("en", set["result"]!["language"]!.GetValue<string>());
            var unknown = await Handle("""{"jsonrpc":"2.0","id":8,"method":"store.loginStart","params":{"store":"nope","redirectUri":"x","state":"y"}}""");
            Assert.Equal("Unknown store: nope", unknown["error"]!["message"]!.GetValue<string>());
            var bad = await Handle("""{"jsonrpc":"2.0","id":9,"method":"system.setLanguage","params":{"language":"fr"}}""");
            Assert.Equal(-32602, bad["error"]!["code"]!.GetValue<int>());
        }
        finally
        {
            await Handle("""{"jsonrpc":"2.0","id":10,"method":"system.setLanguage","params":{"language":"de"}}""");
        }
    }

    [Fact]
    public async Task LoginStartUsesCamelCaseAndLowercaseModes()
    {
        var start = await Handle("""{"jsonrpc":"2.0","id":3,"method":"store.loginStart","params":{"store":"gog","redirectUri":"x","state":"y"}}""");
        Assert.Equal("webview", start["result"]!["mode"]!.GetValue<string>());
        Assert.NotNull(start["result"]!["callbackPrefix"]);
    }

    [Fact]
    public async Task StoreErrorsCarryAKind()
    {
        var response = await Handle("""{"jsonrpc":"2.0","id":4,"method":"store.loginComplete","params":{"store":"gog","code":"abc"}}""");
        Assert.Equal(RpcDispatcher.StoreError, response["error"]!["code"]!.GetValue<int>());
        Assert.Equal("network", response["error"]!["data"]!["kind"]!.GetValue<string>());

        var unknown = await Handle("""{"jsonrpc":"2.0","id":5,"method":"store.loginStart","params":{"store":"origin","redirectUri":"x","state":"y"}}""");
        Assert.Equal("invalidRequest", unknown["error"]!["data"]!["kind"]!.GetValue<string>());
    }

    [Fact]
    public async Task ProtocolErrors()
    {
        Assert.Equal(RpcDispatcher.ParseError, (await Handle("{nope"))["error"]!["code"]!.GetValue<int>());
        Assert.Equal(RpcDispatcher.MethodNotFound, (await Handle("""{"jsonrpc":"2.0","id":6,"method":"nope"}"""))["error"]!["code"]!.GetValue<int>());
        Assert.Equal(RpcDispatcher.InvalidParams, (await Handle("""{"jsonrpc":"2.0","id":7,"method":"store.loginStart","params":{"store":1}}"""))["error"]!["code"]!.GetValue<int>());
        Assert.Null(await Dispatcher.HandleLineAsync("""{"jsonrpc":"2.0","method":"system.ping"}""", TestContext.Current.CancellationToken));
    }
}

public class UnexpectedShapeTests
{
    [Fact]
    public async Task ChangedStoreResponsesAreReportedNotThrown()
    {
        // GOG suddenly answers with an array where an object is expected.
        var dispatcher = RpcDispatcher.CreateDefault(new HttpClient(new StubHandler((_, _) => StubHandler.Json("[1,2,3]"))));
        var line = """{"jsonrpc":"2.0","id":1,"method":"store.library","params":{"session":{"store":"gog","accountId":"1","displayName":"x","accessToken":"a","refreshToken":"r","expiresAt":null},"proxyUrl":null}}""";
        var response = JsonNode.Parse((await dispatcher.HandleLineAsync(line, TestContext.Current.CancellationToken))!)!;
        Assert.Equal(RpcDispatcher.StoreError, response["error"]!["code"]!.GetValue<int>());
        Assert.Equal("unavailable", response["error"]!["data"]!["kind"]!.GetValue<string>());
    }
}
