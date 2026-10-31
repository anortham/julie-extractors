global_plain = 0
global_plain = 1
class Sample
 def writes(initial, box, argument, records)
  initialized = initial
  plain = 0; plain = initial
  compound = 0; compound += initial
  box.value = initial
  consume(argument)
  records.each do |captured|; consume(captured); end
 end
end
