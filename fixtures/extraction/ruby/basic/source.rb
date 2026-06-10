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
end
