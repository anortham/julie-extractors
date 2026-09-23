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

defmodule MyApp.GitHub do
  use Tesla
  plug Tesla.Middleware.BaseUrl, "https://api.github.com"

  def repos, do: get("/user/repos")
end

defmodule MyApp.Aliased do
  alias Req, as: R
  alias MyApp.FakeReq, as: Req

  def real, do: R.get!("https://example.com/real")
  def fake, do: Req.get!("https://example.com/fake")
end
