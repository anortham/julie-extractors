defmodule MyApp.ApiClient do
  @moduledoc "Thin Req HTTP client wrapper."

  def list_users do
    Req.get("https://api.example.com/users")
  end

  def create_user(_payload) do
    Req.post("/users")
  end

  def health do
    Req.get!("/health")
  end

  # Dynamic URLs and deferred clients (HTTPoison/Tesla/Finch/:httpc) are silent.
  def dynamic(id) do
    Req.get("/users/#{id}")
    HTTPoison.get("https://api.example.com/legacy")
  end
end
