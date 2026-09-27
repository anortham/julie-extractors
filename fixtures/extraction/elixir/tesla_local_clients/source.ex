defmodule MyApp.Api do
  def requests(flag) do
    client = Tesla.client([{Tesla.Middleware.BaseUrl, "https://api.example"}])
    Tesla.get(client, "/users")
    Tesla.get(client, "https://override.example/health")
    Tesla.get(client, "relative")
    Tesla.get(client, "")

    client = Tesla.client([{Tesla.Middleware.BaseUrl, base_url: "https://keyword.example/root"}])
    Tesla.get(client, "keyword-relative")
    Tesla.get(client, "https://keyword-override.example/health")

    client =
      Tesla.client([
        {Tesla.Middleware.BaseUrl, base_url: "https://insecure.example/root", policy: :insecure}
      ])
    Tesla.get(client, "insecure-relative")
    Tesla.get(client, "http://insecure-override.example/health")

    client =
      Tesla.client([
        {Tesla.Middleware.BaseUrl, base_url: "https://strict.example/root", policy: :strict}
      ])
    Tesla.get(client, "strict-relative")
    Tesla.get(client, "http://strict-override.example/health")

    client =
      Tesla.client([
        {Tesla.Middleware.BaseUrl,
         base_url: "https://dynamic-policy.example/root", policy: runtime_policy}
      ])
    Tesla.get(client, "/dynamic-policy")

    client =
      Tesla.client([
        {Tesla.Middleware.BaseUrl, base_url: dynamic_base, policy: :strict}
      ])
    Tesla.get(client, "/dynamic-keyword-base")

    client = Tesla.client([{Tesla.Middleware.BaseUrl, "https://replacement.example/"}])
    Tesla.get(client, "/rebound")

    client = Tesla.client([{Tesla.Middleware.BaseUrl, dynamic_base}])
    Tesla.get(client, "/dynamic-base")
    Tesla.get(client, dynamic_path)

    client = Tesla.client([{Tesla.Middleware.BaseUrl, "https://before-branch.example"}])

    if flag do
      client = Tesla.client([{Tesla.Middleware.BaseUrl, "https://branch.example"}])
      Tesla.get(client, "/inside-branch")
    end

    Tesla.get(client, "/after-branch")

    client = Tesla.client([{Tesla.Middleware.BaseUrl, "https://before-outer-if.example"}])
    client =
      if flag do
        Tesla.client([{Tesla.Middleware.BaseUrl, "https://outer-if-then.example"}])
      else
        Tesla.client([{Tesla.Middleware.BaseUrl, "https://outer-if-else.example"}])
      end
    Tesla.get(client, "/after-outer-if")

    client = Tesla.client([{Tesla.Middleware.BaseUrl, "https://before-rebind.example"}])
    client = SomeFactory.client()
    Tesla.get(client, "/unknown-factory")

    client = Tesla.client([{Tesla.Middleware.BaseUrl, "https://recovered.example"}])
    Tesla.get(client, "/after-recovery")

    fn client -> Tesla.get(client, "/shadowed-parameter") end

    case flag do
      client -> Tesla.get(client, "/shadowed-case")
    end

    Tesla.get(client, "/after-shadowing")

    client = Tesla.client([{Tesla.Middleware.BaseUrl, "https://pinned.example"}])
    case client do
      ^client -> Tesla.get(client, "/pinned-case")
    end
    Tesla.get(client, "/after-pinned-case")

    client = Tesla.client([{Tesla.Middleware.BaseUrl, "https://before-nested-module.example"}])

    defmodule MyApp.Api.NestedClient do
      client = Tesla.client([{Tesla.Middleware.BaseUrl, "https://nested-module.example"}])
    end

    Tesla.get(client, "/after-nested-module")
  end

  def request_with_parameter(client) do
    Tesla.get(client, "/parameter")
  end
end

defmodule MyApp.LegacyTesla do
  use Tesla
  plug Tesla.Middleware.BaseUrl, "https://module.example"

  def requests do
    get "/module"
    get "https://module-override.example/health"
  end

  defmodule Unrelated do
    def requests do
      get "/nested-without-tesla-context"
    end
  end
end

defmodule MyApp.StrictLegacyTesla do
  use Tesla
  plug Tesla.Middleware.BaseUrl,
    base_url: "https://module-strict.example/root",
    policy: :strict

  def requests do
    get "module-relative"
    get "http://module-strict-override.example/health"
  end
end
