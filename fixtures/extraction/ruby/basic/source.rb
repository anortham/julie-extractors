class Worker
  DEFAULT_LABEL = "worker"

  def initialize(id)
    @id = id
  end

  def run
    helper(@id)
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
