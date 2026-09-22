# Wave-1 gap fixture: module-scoped call resolution, alias expansion, head
# spans, attribute docs, and keyword structs.
defmodule Fixture.Accounts do
  @moduledoc """
  Account helpers.
  Second line.
  """
  alias Fixture.{Repo, Mailer}
  alias Fixture.Billing.Invoice, as: Inv

  defstruct [:id, name: "anon", role: :user]

  @typedoc "An account id."
  @type id :: integer()

  @doc """
  Factorial with a guard clause.
  """
  @spec fact(integer()) :: integer()
  def fact(0), do: 1
  def fact(n) when n > 0, do: n * fact(n - 1)

  @doc "Totals a list."
  @impl true
  def total(list), do: list |> Enum.sum() |> fact()

  @doc false
  def config, do: Application.get_env(:app, :cfg)

  def start do
    config = config()
    Repo.insert(config)
    Inv.fetch(config)
    Mailer.Queue.push(config)
    __MODULE__.total([1])
    Entry.make()
  end

  def head(opts \\ [])

  defguard is_positive(n) when is_integer(n) and n > 0

  defmacro trace(expr) do
    log_it(expr)
  end

  defp log_it(expr), do: expr

  defmodule Entry do
    @moduledoc "Nested entry."
    defstruct id: nil, tags: [:a, :b]

    def make, do: %__MODULE__{}
  end
end

defmodule Fixture.AccountsTest do
  use ExUnit.Case

  setup do
    {:ok, seed: build_seed()}
  end

  test "totals", %{seed: seed} do
    assert Fixture.Accounts.total([seed]) == 1
  end

  defp build_seed, do: 1
end
