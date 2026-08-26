class Widget
  attr_reader :name
  attr_accessor :size
  delegate :price, to: :vendor

  def save
  end

  def self.find(id)
  end

  class << self
    def build
    end
  end
end
