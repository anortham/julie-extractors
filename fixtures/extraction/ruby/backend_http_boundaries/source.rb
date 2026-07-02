require "net/http"
require "uri"

Rails.application.routes.draw do
  namespace :admin do
    get "/users/:id", to: "users#show", as: :user
    match "/search", via: [:get, :post], to: "search#index"
    resources :accounts, only: [:index, :show]
    mount Sidekiq::Web, at: "/jobs"
  end
  root "home#index"
  namespace :api do
    resources :posts do
      member do
        get "activate"
      end
    end
    get "health", to: "health#show"
  end
end

def call_clients
  Net::HTTP.get(URI("https://api.example.com/users"))
  Net::HTTP.post_form(URI.parse("/items"), { "name" => "x" })
end

get "/outside-draw", to: "legacy#nope"
