namespace Cove.Connectors.Model;

// Amazon installs: mirror the Amazon* types in packages/shared/src/models.ts.

/// <param name="DownloadUrl">Base of the version: <c>/manifest.proto</c> and <c>/files/&lt;hash&gt;</c> hang below it.</param>
public sealed record AmazonDownload(string ProductId, string EntitlementId, string Sku, string VersionId, string DownloadUrl);

/// <summary>Product id → live version id.</summary>
public sealed record AmazonVersions(IReadOnlyDictionary<string, string> Versions);

public sealed record AmazonSdk(string VersionId, string DownloadUrl);
