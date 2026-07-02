using System.Net.Http;
using System.Net.Http.Json;

public class ApiClient
{
    public async Task Load(HttpClient client)
    {
        await client.GetFromJsonAsync<User>("/api/users/1");
        await client.PostAsJsonAsync("https://api.example.com/items", payload);
        var req = new HttpRequestMessage(HttpMethod.Patch, @"/api/users/1");
    }
}
