# Phase 4a fixture: cross-module call. `Router.match` lives in
# Phoenix.Router; the elixir extractor must emit a
# StructuredPendingRelationship with target.terminal_name="match" and
# target.namespace_path=["Phoenix","Router"]. The intra-module call to
# local_helper() resolves concretely.

defmodule Fixture.Worker do
  alias Phoenix.Router

  def entry do
    total =
      if local_helper() > 0 do
        for i <- 1..2, reduce: 0 do
          acc -> acc + i
        end
      else
        0
      end

    Router.match()
    total + local_helper()
  end

  defp local_helper do
    42
  end
end
