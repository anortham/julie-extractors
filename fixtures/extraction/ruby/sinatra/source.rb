require "sinatra/base"

class UsersApp < Sinatra::Base
  before "/admin/*" do
    halt 401 unless authorized?
  end

  get "/users/:id" do
    User.find(params[:id]).to_json
  end

  post "/users" do
    status 201
  end

  delete "/users/:id" do |id|
    User.destroy(id)
  end

  get "/users/#{VERSION}/dynamic" do
    halt 404
  end

  after "/users/*" do
    log_request
  end
end
