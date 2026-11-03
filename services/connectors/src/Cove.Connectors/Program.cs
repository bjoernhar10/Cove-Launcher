using Cove.Connectors.Rpc;

// stdout carries JSON-RPC only; all logging goes to stderr.
await Console.Error.WriteLineAsync($"cove-connectors {RpcDispatcher.Version} ready").ConfigureAwait(false);

while (await Console.In.ReadLineAsync().ConfigureAwait(false) is { } line)
{
    if (string.IsNullOrWhiteSpace(line))
    {
        continue;
    }

    if (RpcDispatcher.HandleLine(line) is { } response)
    {
        await Console.Out.WriteLineAsync(response).ConfigureAwait(false);
        await Console.Out.FlushAsync().ConfigureAwait(false);
    }
}
