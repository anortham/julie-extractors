defmodule Shop.Store do
  defstruct [:path]
  @type t :: %__MODULE__{}

  @spec new() :: t()
  def new, do: %__MODULE__{}

  @spec open(String.t()) :: {:ok, Conn.t()} | {:error, term()}
  def open(path), do: {:ok, path}

  @spec load() :: Workspace.t()
  def load, do: nil
end

defmodule Shop.Web do
  alias Shop.Store

  def run(path) do
    ws = Store.load()
    store = Store.new()
    {:ok, conn} = path |> Store.open()
    other = External.load()
    {ws, store, conn, other}
  end
end
