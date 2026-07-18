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

  # Dynamic URLs stay silent (M2).
  def dynamic(id) do
    Req.get("/users/#{id}")
  end
end
