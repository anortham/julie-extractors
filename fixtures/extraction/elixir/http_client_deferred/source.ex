defmodule MyApp.DeferredClient do
  @moduledoc "Deferred Elixir HTTP clients fixture."

  def load(client) do
    Tesla.get!("https://api.example.com/tesla")
    Tesla.post(client, "/tesla/items", "body")
    HTTPoison.get("/httpoison/users")
    HTTPoison.request!(:delete, "/httpoison/1", "", [], [])
    Finch.build(:get, "https://api.example.com/finch")
    :httpc.request("https://api.example.com/httpc")
    :httpc.request(:post, {'/httpc/items', []}, [], [])

    # Silent dynamics / non-clients.
    Tesla.get("/users/#{id}")
    Finch.request(req, MyFinch)
    OtherClient.get("/nope")
  end
end
