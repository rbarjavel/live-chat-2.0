extends RichTextLabel

func _ready():
	# Enable BBCode
	bbcode_enabled = true
	
	add_theme_constant_override("outline_size", 5)
	add_theme_color_override("font_outline_color", Color(0, 0, 0, 1))  # Black outline

	# Connect to window resize events - Godot 4.x syntax
	get_window().size_changed.connect(update_text_scale)
	
	var args = OS.get_cmdline_args()
	if args.size() > 0:
		var input_text = args[1]
		update_text_scale(input_text)
	else:
		update_text_scale("")

func update_text_scale(custom_text: String = ""):
	var font_size = 66
	var text_content = custom_text if custom_text != "" else ""
	text = "[center][font name=\"res://fonts/impact.ttf\" size=" + str(font_size) + "]" + text_content + "[/font][/center]"
