class Polygon
  def area
    vertices.each_cons(2).sum { |a, b| a.x * b.y - b.x * a.y }.abs / 2.0
  end

  def perimeter
    vertices.each_cons(2).sum { |a, b| Math.hypot(b.x - a.x, b.y - a.y) }
  end

  def centroid
    Point.new(vertices.sum(&:x) / vertices.size, vertices.sum(&:y) / vertices.size)
  end
end

class Trail
  # Same computation as Polygon#perimeter, copied rather than shared.
  def length
    vertices.each_cons(2).sum { |a, b| Math.hypot(b.x - a.x, b.y - a.y) }
  end
end
