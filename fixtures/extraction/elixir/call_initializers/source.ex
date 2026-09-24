defmodule Shop.Store do
  @spec open(String.t()) :: {:ok, Conn.t()} | {:error, term()}
  def open(path), do: {:ok, path}

  @spec load() :: Workspace.t()
  def load, do: nil
end

defmodule Shop.Web do
  alias Shop.Store

  def run(path) do
    ws = Store.load()
    {:ok, conn} = path |> Store.open()
    other = External.load()
    {ws, conn, other}
  end
end
