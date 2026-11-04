using System.Net;
using System.Text.Json;
using System.Text.Json.Nodes;
using Cove.Connectors.Model;

namespace Cove.Connectors.Http;

/// <summary>Sends store requests and turns every failure into a <see cref="ConnectorException"/>.</summary>
public static class StoreHttp
{
    public static async Task<string> SendTextAsync(HttpClient client, HttpRequestMessage request, string store, CancellationToken ct)
    {
        HttpResponseMessage response;
        try
        {
            response = await client.SendAsync(request, ct).ConfigureAwait(false);
        }
        catch (HttpRequestException e)
        {
            throw new ConnectorException(ConnectorErrorKind.Network, $"{store} ist nicht erreichbar.", e);
        }
        catch (TaskCanceledException e) when (!ct.IsCancellationRequested)
        {
            throw new ConnectorException(ConnectorErrorKind.Network, $"{store} antwortet nicht (Zeitüberschreitung).", e);
        }

        using (response)
        {
            var body = await response.Content.ReadAsStringAsync(ct).ConfigureAwait(false);
            if (response.IsSuccessStatusCode)
            {
                return body;
            }

            throw response.StatusCode switch
            {
                HttpStatusCode.Unauthorized or HttpStatusCode.Forbidden =>
                    new ConnectorException(ConnectorErrorKind.AuthFailed, $"{store} hat die Anmeldung abgelehnt."),
                HttpStatusCode.BadRequest when body.Contains("invalid_grant", StringComparison.OrdinalIgnoreCase) ||
                                               body.Contains("errors.com.epicgames.account.oauth", StringComparison.OrdinalIgnoreCase) =>
                    new ConnectorException(ConnectorErrorKind.AuthFailed, $"{store} hat die Anmeldung abgelehnt."),
                HttpStatusCode.TooManyRequests =>
                    new ConnectorException(ConnectorErrorKind.Unavailable, $"{store} meldet zu viele Anfragen, bitte später erneut versuchen."),
                _ => new ConnectorException(ConnectorErrorKind.Unavailable, $"{store} ist gerade nicht verfügbar (HTTP {(int)response.StatusCode})."),
            };
        }
    }

    public static async Task<JsonNode> SendJsonAsync(HttpClient client, HttpRequestMessage request, string store, CancellationToken ct)
    {
        var body = await SendTextAsync(client, request, store, ct).ConfigureAwait(false);
        try
        {
            return JsonNode.Parse(body) ?? throw new JsonException("empty");
        }
        catch (JsonException e)
        {
            throw new ConnectorException(ConnectorErrorKind.Unavailable, $"{store} hat unerwartet geantwortet.", e);
        }
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
