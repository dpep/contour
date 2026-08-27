class Depot
  def claim(item, amount)
    supply = lookup(item)
    supply.hold(amount)
    supply
  end
end
