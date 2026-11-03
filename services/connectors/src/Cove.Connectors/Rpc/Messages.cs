using System.Text.Json.Serialization;

namespace Cove.Connectors.Rpc;

// Shapes mirror packages/shared/src/methods.ts — keep them in sync.

public sealed record PingParams(string? Message);

public sealed record PingResult(string Service, string Version, string Message);

/// <summary>Source-generated JSON metadata (required for Native AOT — no reflection).</summary>
[JsonSourceGenerationOptions(
    PropertyNamingPolicy = JsonKnownNamingPolicy.CamelCase,
    DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull)]
[JsonSerializable(typeof(PingParams))]
[JsonSerializable(typeof(PingResult))]
internal sealed partial class RpcJsonContext : JsonSerializerContext
{
}
