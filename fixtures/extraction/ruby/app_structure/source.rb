#!/usr/bin/env ruby
# frozen_string_literal: true

# JSON encoding for API payloads.
require "json"

module Api::V1
  # Base for versioned API controllers.
  class BaseController < ApplicationController
    # Default page size.
    PAGE_SIZE = 25

    # Renders the payload as JSON.
    def respond(payload)
      render json: payload.to_json, status: :ok
    end
  end

  # rubocop:disable Metrics/ClassLength

  class OrdersController < Api::V1::BaseController
    # Current order total.
    attr_reader :amount

    def total
      subtotal + tax
    end

    def subtotal
      items.sum(&:price)
    end

    def tax
      rate = tax_rate
      subtotal * rate
    end

    def export(rows)
      rows.each { |row| send(:audit, row) }
      Mailer.with(to: owner).receipt.deliver_later
      self.status = :exported
    end

    def classify(value)
      case value
      when Hash then PAGE_SIZE
      end
    rescue TypeError => e
      warn e.message
      $stderr.puts e
    end

    def respond(payload)
      super
    end

    def audit(row)
      row
    end

    def tax_rate
      0.1
    end

    def items
      []
    end
  end
end
