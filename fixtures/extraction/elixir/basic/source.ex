defmodule Fixture.Worker do
  @moduledoc "Worker helpers for fixture extraction."
  @spec run(integer()) :: integer()

  import Kernel, only: [apply: 2]
  alias Fixture.Helper
  require Logger

  def run(id) do
    record_run(id)
    helper(id)
  end

  @doc "Increment a worker id."
  defp helper(value) do
    value + 1
  end

  defp record_run(id) do
    observe_run("worker-run", id)
  end

  defp observe_run(_event, _id), do: :ok

  @doc "Checks the worker service health endpoint."
  def fetch_status do
    fetch_url("https://api.example.com/workers/status")
  end

  defp fetch_url(_url), do: :ok

  def piped(id), do: id |> helper() |> Kernel.abs()

  def safe_div(a, b) do
    with true <- b != 0 do
      div(a, b)
    else
      _ -> 0
    end
  end

  def evaluate(count, enabled) do
    if enabled do
      for i <- 1..count, reduce: 0 do
        acc -> acc + i
      end
    else
      if count > 0, do: 1, else: 0
    end
  end
end
