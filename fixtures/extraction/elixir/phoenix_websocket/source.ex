defmodule MyAppWeb.Endpoint do
  use Phoenix.Endpoint, otp_app: :my_app

  socket "/socket", MyAppWeb.UserSocket,
    websocket: true,
    longpoll: false

  socket @dynamic_path, MyAppWeb.DynamicSocket
  socket "/dynamic-handler", @dynamic_handler

  defmodule Nested do
    socket "/nested-unattested", MyAppWeb.NestedSocket
  end
end

defmodule MyAppWeb.UserSocket do
  use Phoenix.Socket

  channel "room:*", MyAppWeb.RoomChannel, assigns: %{role: :member}
  channel @dynamic_topic, MyAppWeb.DynamicChannel
  channel "dynamic-handler", @dynamic_handler

  defmodule NestedChannel do
    use Phoenix.Socket

    channel "nested:*", MyAppWeb.NestedChannelHandler
  end
end

defmodule MyAppWeb.AliasedEndpoint do
  alias Phoenix.Endpoint
  use Endpoint, otp_app: :my_app

  socket "/aliased", MyAppWeb.AliasedSocket
end

defmodule MyAppWeb.ImportedSocket do
  import Phoenix.Socket

  channel "imported:*", MyAppWeb.ImportedChannel
end

defmodule MyAppWeb.Router do
  use Phoenix.Router

  pipeline :api do
    plug :accepts, ["json"]
  end

  pipe_through :api
  get "/health", MyAppWeb.HealthController, :show
end

defmodule MyAppWeb.Unrelated do
  def socket(path, handler), do: {path, handler}

  def channel(topic, handler), do: {topic, handler}

  def pretend do
    socket "/fake", MyAppWeb.FakeSocket
    channel "fake:*", MyAppWeb.FakeChannel
  end
end
