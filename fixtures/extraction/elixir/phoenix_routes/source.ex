defmodule MyAppWeb.Router do
  use Phoenix.Router

  pipeline :api do
    plug :accepts, ["json"]
  end

  scope "/api", MyAppWeb do
    pipe_through :api

    get "/users/:id", UserController, :show
    post "/users", UserController, :create
    put "/users/:id", UserController, :update
    delete "/users/:id", UserController, :destroy

    resources "/photos", PhotoController

    scope "/v1" do
      get "/status", StatusController, :index
    end

    forward "/health", HealthPlug
  end

  # Dynamic route args are silent (M2 silence): interpolation, concatenation,
  # and the `~r` regex sigil never emit a route fact.
  get "/tenants/#{tenant}", TenantController, :show
  get "/legacy/" <> path, LegacyController, :show
  get ~r"/regex", RegexController, :show
end
