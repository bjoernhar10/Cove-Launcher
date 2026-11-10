using System.Net;
using System.Text.Json;
using System.Text.Json.Nodes;
using Cove.Connectors.Model;

namespace Cove.Connectors.Http;

/// <summary>Sends store requests and turns every failure into a <see cref="ConnectorException"/>.</summary>
public static class StoreHttp
{
    private static async Task<HttpResponseMessage> SendAsync(HttpClient client, HttpRequestMessage request, string store, CancellationToken ct)
    {
        try
        {
            return await client.SendAsync(request, ct).ConfigureAwait(false);
        }
        catch (HttpRequestException e)
        {
            throw new ConnectorException(ConnectorErrorKind.Network, Texts.T($"{store} ist nicht erreichbar.", $"{store} can't be reached."), e);
        }
        catch (TaskCanceledException e) when (!ct.IsCancellationRequested)
        {
            throw new ConnectorException(ConnectorErrorKind.Network, Texts.T($"{store} antwortet nicht (Zeitüberschreitung).", $"{store} isn't responding (timeout)."), e);
        }
    }

    private static ConnectorException StatusError(HttpResponseMessage response, string body, string store) => response.StatusCode switch
    {
        HttpStatusCode.Unauthorized or HttpStatusCode.Forbidden =>
            new ConnectorException(ConnectorErrorKind.AuthFailed, $"{store} hat die Anmeldung abgelehnt."),
        HttpStatusCode.BadRequest when body.Contains("invalid_grant", StringComparison.OrdinalIgnoreCase) ||
                                       body.Contains("errors.com.epicgames.account.oauth", StringComparison.OrdinalIgnoreCase) =>
            new ConnectorException(ConnectorErrorKind.AuthFailed, $"{store} hat die Anmeldung abgelehnt."),
        HttpStatusCode.TooManyRequests =>
            new ConnectorException(ConnectorErrorKind.Unavailable, Texts.T($"{store} meldet zu viele Anfragen, bitte später erneut versuchen.", $"{store} reports too many requests. Please try again later.")),
        _ => new ConnectorException(ConnectorErrorKind.Unavailable, Texts.T($"{store} ist gerade nicht verfügbar (HTTP {(int)response.StatusCode}).", $"{store} is not available right now (HTTP {(int)response.StatusCode}).")),
    };

    public static async Task<string> SendTextAsync(HttpClient client, HttpRequestMessage request, string store, CancellationToken ct)
    {
        using var response = await SendAsync(client, request, store, ct).ConfigureAwait(false);
        var body = await response.Content.ReadAsStringAsync(ct).ConfigureAwait(false);
        return response.IsSuccessStatusCode ? body : throw StatusError(response, body, store);
    }

    /// <summary>Binary answers (e.g. Epic ownership tokens).</summary>
    public static async Task<byte[]> SendBytesAsync(HttpClient client, HttpRequestMessage request, string store, CancellationToken ct)
    {
        using var response = await SendAsync(client, request, store, ct).ConfigureAwait(false);
        var body = await response.Content.ReadAsByteArrayAsync(ct).ConfigureAwait(false);
        return response.IsSuccessStatusCode ? body : throw StatusError(response, System.Text.Encoding.UTF8.GetString(body), store);
    }

    public static async Task<JsonNode> SendJsonAsync(HttpClient client, HttpRequestMessage request, string store, CancellationToken ct)
    {
        var body = await SendTextAsync(client, request, store, ct).ConfigureAwait(false);
        return Parse(body, store);
    }

    private static JsonNode Parse(string body, string store)
    {
        try
        {
            return JsonNode.Parse(body) ?? throw new JsonException("empty");
        }
        catch (JsonException e)
        {
            throw new ConnectorException(ConnectorErrorKind.Unavailable, $"{store} hat unerwartet geantwortet.", e);
        }
    }

    /// <summary>Like <see cref="GetJsonAsync"/>, but null when the resource doesn't exist (404).</summary>
    public static async Task<JsonNode?> GetJsonOrNullAsync(HttpClient client, string url, string store, CancellationToken ct, string? bearer = null)
    {
        using var request = new HttpRequestMessage(HttpMethod.Get, url);
        if (bearer is not null)
        {
            request.Headers.Authorization = new System.Net.Http.Headers.AuthenticationHeaderValue("Bearer", bearer);
        }

        using var response = await SendAsync(client, request, store, ct).ConfigureAwait(false);
        if (response.StatusCode == HttpStatusCode.NotFound)
        {
            return null;
        }

        var body = await response.Content.ReadAsStringAsync(ct).ConfigureAwait(false);
        return response.IsSuccessStatusCode ? Parse(body, store) : throw StatusError(response, body, store);
    }

    public static Task<JsonNode> GetJsonAsync(HttpClient client, string url, string store, CancellationToken ct, string? bearer = null)
    {
        var request = new HttpRequestMessage(HttpMethod.Get, url);
        if (bearer is not null)
        {
            request.Headers.Authorization = new System.Net.Http.Headers.AuthenticationHeaderValue("Bearer", bearer);
        }

        return SendJsonAsync(client, request, store, ct);
    }

    /// <summary>Required string field or an "unexpected answer" error (the store API changed).</summary>
    public static string Required(JsonNode? node, string field, string store)
    {
        var value = node?[field];
        return value is JsonValue v && v.TryGetValue(out string? s) && !string.IsNullOrEmpty(s)
            ? s
            : value is JsonValue n && n.TryGetValue(out long l)
                ? l.ToString(System.Globalization.CultureInfo.InvariantCulture)
                : throw new ConnectorException(ConnectorErrorKind.Unavailable, $"{store} hat unerwartet geantwortet (fehlendes Feld „{field}“).");
    }

    public static string? Optional(JsonNode? node, string field)
    {
        var value = node?[field];
        if (value is not JsonValue v)
        {
            return null;
        }

        if (v.TryGetValue(out string? s))
        {
            return s;
        }

        return v.TryGetValue(out long l) ? l.ToString(System.Globalization.CultureInfo.InvariantCulture) : null;
    }

    public static long? OptionalLong(JsonNode? node, string field)
    {
        var value = node?[field];
        return value is JsonValue v && v.TryGetValue(out long l) ? l
            : value is JsonValue d && d.TryGetValue(out double x) ? (long)x
            : null;
    }
}
