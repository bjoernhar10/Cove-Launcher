using System.Text.Json.Nodes;
using Cove.Connectors.Config;
using Cove.Connectors.Http;
using Cove.Connectors.Local;
using Cove.Connectors.Model;

namespace Cove.Connectors.Connectors;

/// <summary>
/// GOG: OAuth2 on auth.gog.com with the public Galaxy client (as Heroic/gogdl).
/// The core shows the login in a webview and catches the redirect to
/// embed.gog.com/on_login_success to read the code.
/// </summary>
public sealed class GogConnector(HttpClient http, GogConfig config) : IStoreConnector
{
    public string Id => "gog";

    public string Name => "GOG";

    public LoginStart LoginStart(string redirectUri, string state)
    {
        var url = $"{config.AuthUrl}?client_id={config.ClientId}&redirect_uri={Uri.EscapeDataString(config.RedirectUri)}" +
                  "&response_type=code&layout=client2";
        var callback = config.RedirectUri.Split('?')[0];
        return new LoginStart(Id, url, LoginMode.Webview, callback, null);
    }

    private async Task<StoreSession> TokenAsync(string grant, string? displayName, CancellationToken ct)
    {
        var url = $"{config.TokenUrl}?client_id={config.ClientId}&client_secret={config.ClientSecret}&{grant}";
        var body = await StoreHttp.GetJsonAsync(http, url, Name, ct).ConfigureAwait(false);
        var access = StoreHttp.Required(body, "access_token", Name);
        var userId = StoreHttp.Required(body, "user_id", Name);
        var expiresIn = StoreHttp.OptionalLong(body, "expires_in") ?? 3600;
        displayName ??= await UserNameAsync(access, ct).ConfigureAwait(false) ?? userId;
        return new StoreSession(
            Id, userId, displayName, access, StoreHttp.Required(body, "refresh_token", Name), DateTimeOffset.UtcNow.AddSeconds(expiresIn));
    }

    private async Task<string?> UserNameAsync(string accessToken, CancellationToken ct)
    {
        try
        {
            var data = await StoreHttp.GetJsonAsync(http, config.UserDataUrl, Name, ct, accessToken).ConfigureAwait(false);
            return StoreHttp.Optional(data, "username");
        }
        catch (ConnectorException)
        {
            return null;
        }
    }

    public Task<StoreSession> LoginCompleteAsync(LoginCompleteRequest request, StoreContext context, CancellationToken ct)
    {
        if (string.IsNullOrWhiteSpace(request.Code))
        {
            throw new ConnectorException(ConnectorErrorKind.InvalidRequest, "GOG hat keinen Anmeldecode geliefert.");
        }

        var grant = $"grant_type=authorization_code&code={Uri.EscapeDataString(request.Code.Trim())}" +
                    $"&redirect_uri={Uri.EscapeDataString(config.RedirectUri)}";
        return TokenAsync(grant, null, ct);
    }

    public Task<StoreSession> RefreshAsync(StoreSession session, CancellationToken ct)
    {
        if (session.RefreshToken is null)
        {
            throw new ConnectorException(ConnectorErrorKind.AuthFailed, "Bitte melde dich erneut bei GOG an.");
        }

        return TokenAsync($"grant_type=refresh_token&refresh_token={Uri.EscapeDataString(session.RefreshToken)}", session.DisplayName, ct);
    }

    public async Task<IReadOnlyList<OwnedGame>> GetLibraryAsync(StoreSession session, StoreContext context, CancellationToken ct)
    {
        var games = new List<OwnedGame>();
        for (var page = 1; page <= 50; page++)
        {
            var url = config.OwnedProductsUrl.Replace("{page}", page.ToString(System.Globalization.CultureInfo.InvariantCulture), StringComparison.Ordinal);
            var body = await StoreHttp.GetJsonAsync(http, url, Name, ct, session.AccessToken).ConfigureAwait(false);
            foreach (var product in body["products"]?.AsArray() ?? [])
            {
                var image = StoreHttp.Optional(product, "image");
                games.Add(new OwnedGame(
                    StoreHttp.Required(product, "id", Name),
                    StoreHttp.Optional(product, "title") ?? "Unbekanntes Spiel",
                    null,
                    null,
                    image is null ? null : $"https:{image}.jpg"));
            }

            if (page >= (StoreHttp.OptionalLong(body, "totalPages") ?? 1))
            {
                break;
            }
        }

        return games;
    }

    /// <summary>GOG achievements need the Galaxy gameplay API per game; not supported yet.</summary>
    public Task<IReadOnlyList<StoreAchievement>> GetAchievementsAsync(
        StoreSession session, string storeGameId, StoreContext context, CancellationToken ct) =>
        Task.FromResult<IReadOnlyList<StoreAchievement>>([]);

    public IReadOnlyList<LocalGame> DetectLocal(LocalEnvironment environment) =>
        [.. LocalDetection.DetectAll(environment).Where(g => g.Store == Id)];
}
