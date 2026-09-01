require "json"
require_relative "./helper"

class Widget
end

class Worker
  include Enumerable

  DEFAULT_LABEL = "worker"

  def initialize(id)
    @id = id
  end

  def reset
    @id = 0
  end

  def assemble(a, b = 1, *rest, key:, &blk)
    w = Widget.new
    u = Unknown.new
    n = Net::HTTP.new
    v = build
    self.helper
    self.missing_wave2
  end

  def run
    [1, 2].map { |value| helper(value) }
  end

  def risky
    1 / 0
  rescue ZeroDivisionError
    0
  end

  private

  # Increments a worker id.
  def helper(value)
    value + 1
  end

  def evaluate(count, enabled)
    total = 0
    if enabled
      for i in 0...count
        total += i
      end
    end
    until total >= count
      total += 1
    end
    total
  end
end
