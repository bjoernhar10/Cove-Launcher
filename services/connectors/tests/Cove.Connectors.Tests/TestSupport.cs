using System.Net;
using System.Text;
using Cove.Connectors.Config;

namespace Cove.Connectors.Tests;

/// <summary>Answers HTTP requests from a function and records them.</summary>
internal sealed class StubHandler(Func<HttpRequestMessage, string, HttpResponseMessage> respond) : HttpMessageHandler
{
    public List<(HttpRequestMessage Request, string Body)> Requests { get; } = [];

    protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
    {
        var body = request.Content is null ? string.Empty : await request.Content.ReadAsStringAsync(cancellationToken).ConfigureAwait(false);
        lock (Requests)
        {
            Requests.Add((request, body));
        }

        return respond(request, body);
    }

    public static HttpResponseMessage Json(string json, HttpStatusCode status = HttpStatusCode.OK) =>
        new(status) { Content = new StringContent(json, Encoding.UTF8, "application/json") };

    public static HttpResponseMessage Text(string text, HttpStatusCode status = HttpStatusCode.OK) =>
        new(status) { Content = new StringContent(text) };
}

/// <summary>A stub that can wait, for tests about requests running at the same time.</summary>
internal sealed class AsyncStubHandler(Func<HttpRequestMessage, Task<HttpResponseMessage>> respond) : HttpMessageHandler
{
    private int calls;

    /// <summary>Catalogue requests seen (everything but the library list).</summary>
    public int Calls => calls;

    protected override Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken cancellationToken)
    {
        if (request.RequestUri!.AbsolutePath.Contains("/namespace/", StringComparison.Ordinal))
        {
            Interlocked.Increment(ref calls);
        }

        return respond(request);
    }
}

internal static class TestConfig
{
    public static ConnectorConfig Default { get; } = ConnectorConfig.Load();
}

/// <summary>A throw-away directory tree for manifest fixtures.</summary>
internal sealed class TempTree : IDisposable
{
    public string Root { get; } = Directory.CreateTempSubdirectory("cove-connectors-").FullName;

    public string Write(string relative, string content)
    {
        var path = Path.Combine(Root, relative);
        Directory.CreateDirectory(Path.GetDirectoryName(path)!);
        File.WriteAllText(path, content);
        return path;
    }

    public void Dispose() => Directory.Delete(Root, recursive: true);
}
