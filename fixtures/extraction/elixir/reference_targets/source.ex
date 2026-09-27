defmodule Engine do
  import Enum, only: [map: 2]
  def map(items), do: items
  def run_local, do: map([])
  def run_imported, do: map([], fn item -> item end)
  def run_piped, do: [] |> map(fn item -> item end)
  def capture_imported, do: &map/2
end

defmodule Worker do
  def work(value, options \\ []), do: {value, options}
end

defmodule Consumer do
  def good, do: Worker.work(1)
  def wrong_arity, do: Worker.work(1, 2, 3)
  defdelegate delegated(value), to: Worker, as: :work
  defdelegate wrong_delegate(a, b, c), to: Worker, as: :work
end
