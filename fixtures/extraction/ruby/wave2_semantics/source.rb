require "json"
require "faraday"

module Payments
  class Base; end

  class Card < Base
    include Auditable, Trackable
    extend Forwardable

    SEPARATORS = %w[, ;]
    PATTERN = %r{\d+}

    def_delegator :@ledger, :first, :opening_entry
    def_delegators :@ledger, :size, :each
    delegate :email, to: :owner

    class << self
      def load(id)
        new(id)
      end
    end

    def self.[](key); registry.fetch(key); end
    def self.logger=(value); @logger = value; end

    def initialize(ledger)
      @ledger = ledger
      @client = Base.new
    end

    def total
      @ledger.sum
    end
    alias sum total
    alias_method :grand_total, :total
    define_method(:reset!) { @ledger.clear }

    def charge(amount)
      counter = 0
      counter += amount
      @ledger.record(counter)
    rescue Timeout::Error, IOError => e
      retry_later(e)
    rescue => e
      log(e)
    end

    def compute; end
    def self.build; new([]); end
    private :compute
    private_class_method :build
    private attr_reader :token
  end

  Error = Class.new(StandardError)
  DeclinedError = Class.new(Error) do
    def retryable?
      false
    end
  end
  Coord = Data.define(:lat, :lng)
end

class User < ApplicationRecord
  has_many :orders
  belongs_to :account
  scope :active, -> { where("active = ?", true).order("created_at DESC") }
  before_save :normalize_email
  validate :email_present, if: :email_required?

  def self.count_orders
    connection.execute(<<~SQL)
      SELECT count(*)
      FROM orders
    SQL
  end

  private

  def normalize_email; end
  def email_present; end
  def email_required?; end
end

class PostsController < ApplicationController
  before_action :set_post, only: %i[show]
  rescue_from ActiveRecord::RecordNotFound, with: :render_not_found
  helper_method :current_author

  def show
    send(:track_view)
    params.require(:post).permit(:title)
  end

  def fetch_remote
    HTTParty.get("https://api.example.com/v1/reports")
    RestClient.post "https://hooks.example.com/posts", payload
    conn = Faraday.new(url: "https://svc.example.com")
    conn.get("/items")
    uri = URI("https://x.example.com/feed")
    Net::HTTP.get(uri)
  end

  private

  def set_post; end
  def track_view; end
  def render_not_found; end
  def current_author; end
end
