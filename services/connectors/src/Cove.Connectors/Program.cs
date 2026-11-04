using Cove.Connectors.Rpc;

// stdout carries JSON-RPC only; all logging goes to stderr. Tokens arrive per call and are never logged.
using var http = new HttpClient { Timeout = TimeSpan.FromSeconds(20) };
http.DefaultRequestHeaders.UserAgent.ParseAdd($"Cove/{RpcDispatcher.Version}");
var dispatcher = RpcDispatcher.CreateDefault(http);

await Console.Error.WriteLineAsync($"cove-connectors {RpcDispatcher.Version} ready").ConfigureAwait(false);

while (await Console.In.ReadLineAsync().ConfigureAwait(false) is { } line)
{
    if (string.IsNullOrWhiteSpace(line))
    {
        continue;
    }

    if (await dispatcher.HandleLineAsync(line).ConfigureAwait(false) is { } response)
    {
        await Console.Out.WriteLineAsync(response).ConfigureAwait(false);
        await Console.Out.FlushAsync().ConfigureAwait(false);
    }
}
