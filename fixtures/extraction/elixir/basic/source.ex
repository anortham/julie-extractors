defmodule Fixture.Worker do
  @spec run(integer()) :: integer()
  def run(id) do
    record_run(id)
    helper(id)
  end

  defp helper(value) do
    value + 1
  end

  defp record_run(id) do
    observe_run("worker-run", id)
  end

  defp observe_run(_event, _id), do: :ok

  def fetch_status do
    fetch_url("https://api.example.com/workers/status")
  end

  defp fetch_url(_url), do: :ok
end
