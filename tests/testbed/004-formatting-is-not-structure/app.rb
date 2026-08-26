class Alpha
  def render(rows)
    label = "total"
    rows.map { |row| format(label, row, 1000) }
  end
end

class Beta
  def render(rows)
    # Reformatted, recommented, requoted, and the number respelled. Prism
    # sees through all four, which is why the vendored atom table earns its
    # keep: none of this needed a special case.
    label = 'total'
    rows.map { |row|
      format(
        label,
        row,
        1_000,
      )
    }
  end
end

class Gamma
  def render(rows)
    # A different literal is a different method. This is where normalization
    # stops.
    label = "subtotal"
    rows.map { |row| format(label, row, 1000) }
  end
end
