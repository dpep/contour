class Warehouse
  def reserve(sku, count)
    stock = lookup(sku)
    stock.hold(count)
    stock
  end
end

class Depot
  def reserve(sku, count)
    stock = lookup(sku)
    stock.hold(count)
    stock
  end
end
