module Alpha
  module Beta
    def run
    end
  end
end

module Alpha::Gamma
  def run
  end
end

module Delta
  module_function

  def helper
  end
end
