defmodule Shop.Cart do
  @moduledoc "Cart operations."
  @timeout 5_000
  @default_opts [retries: 3]

  @type result(t) :: {:ok, t} | {:error, term()}
  @spec totals(Enumerable.t(Item.t())) :: MapSet.t(integer())
  def totals(items), do: MapSet.new(Enum.map(items, &price/1))

  @spec fetch(integer()) :: Item.t()
  def fetch(id), do: id
  @spec fetch(integer(), keyword()) :: Account.t()
  def fetch(id, _opts), do: id

  @spec count :: non_neg_integer()
  def count, do: 0

  def run(cart, mod, opts \\ @default_opts) do
    if cart.user.admin? do
      case :ets.lookup(:carts, cart.id) do
        [] -> mod.process(cart, @timeout)
        found -> found
      end
    else
      apply_fn = fn x -> x end
      apply_fn.(opts)
    end
  end

  defp price(item), do: item

  defdelegate list_users(opts), to: Shop.Accounts
  defdelegate lookup(id), to: Shop.Cart.Store, as: :get

  defmacro trace(expr), do: expr
end

defmodule Shop.Cart.Store do
  def get(id), do: id
end

defprotocol Shape do
  @macrocallback shape_macro(term()) :: Macro.t()
  def area(shape)
end

defmodule Circle do
  defimpl Shape do
    def area(_circle), do: 1
  end
end

defimpl Shape, for: [Map, List] do
  def area(_), do: 0
end

defimpl Jason.Encoder, for: Circle do
  def encode(circle, opts), do: Jason.Encode.map(circle, opts)
end

defmodule Shop.Item do
  use Ecto.Schema

  schema "items" do
    field :name, :string
    belongs_to :cart, Shop.Cart
    has_many :tags, Shop.Tag
    timestamps()
  end
end

defmodule Shop.Page do
  def render(assigns) do
    ~H"""
    <div>{@title}</div>
    """
  end

  def email?(s), do: s =~ ~r/^[^@]+@[^@]+$/
end
