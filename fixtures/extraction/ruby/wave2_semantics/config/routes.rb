Rails.application.routes.draw do
  get "login" => "sessions#new"
  resources :users, only: %i[index show] do
    member do
      post :activate
    end
    collection do
      get :search
    end
    get :receipt, on: :member
    resources :posts, only: :index
  end
  get "/reports/:id",
      to: "reports#show",
      as: :report
  namespace :admin do resources :audits end
  namespace :api, defaults: { format: :json } do
    resources :orders
  end
end
