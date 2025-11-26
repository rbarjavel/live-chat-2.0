extends Node2D

# Helper function to create animated wave text
func create_wave_text(text: String, position: Vector2, max_width: int) -> Control:
	var container = Control.new()
	container.name = "WaveTextContainer"
	container.position = position
	container.size = Vector2(float(max_width), 150)

	# Load custom font
	var font = load("res://fonts/impact.ttf")
	var font_size = 48

	# Calculate total text width to center it
	var char_width = 30  # Approximate width per character
	var total_width = text.length() * char_width
	var start_x = (float(max_width) - float(total_width)) / 2.0

	# Create a label for each character
	for i in range(text.length()):
		var char_label = Label.new()
		char_label.text = text[i]
		char_label.add_theme_font_override("font", font)
		char_label.add_theme_font_size_override("font_size", font_size)
		char_label.add_theme_color_override("font_color", Color(1, 1, 1, 1))
		char_label.add_theme_color_override("font_outline_color", Color(0, 0, 0, 1))
		char_label.add_theme_constant_override("outline_size", 5)

		# Position character
		char_label.position = Vector2(start_x + float(i) * float(char_width), 0)

		# Create animation using a Tween
		var tween = create_tween()
		tween.set_loops()
		var delay = float(i) * 0.1  # Stagger animation for wave effect

		# Wave animation: move up and down
		tween.tween_property(char_label, "position:y", -20.0, 0.5).set_delay(delay).set_trans(Tween.TRANS_SINE).set_ease(Tween.EASE_IN_OUT)
		tween.tween_property(char_label, "position:y", 0.0, 0.5).set_trans(Tween.TRANS_SINE).set_ease(Tween.EASE_IN_OUT)

		container.add_child(char_label)

	return container

func _ready():
	# Set viewport background to transparent
	get_tree().root.get_viewport().transparent_bg = true

	var args = OS.get_cmdline_args()
	print("Command line args: ", args)
	if args.size() > 0:
		var file_path = args[0]
		var media_type = "video"  # default to video
		var caption_text = ""
		var width = 1920
		var height = 1080

		# Parse arguments: <file_path> <media_type> [caption] [width] [height]
		if args.size() > 1:
			# Check if arg[1] is media type or caption
			if args[1] == "image" or args[1] == "video":
				media_type = args[1]
				if args.size() > 2:
					caption_text = args[2]
				if args.size() >= 5:
					width = args[3].to_int()
					height = args[4].to_int()
			else:
				# Old format: arg[1] is caption
				caption_text = args[1]
				if args.size() >= 4:
					width = args[2].to_int()
					height = args[3].to_int()

		if media_type == "image":
			play_image(file_path, caption_text, width, height)
		else:
			play_video(file_path, caption_text, width, height)
	else:
		push_error("No media file provided")
		OS.kill(OS.get_process_id())

func play_video(video_path: String, caption_text: String, actual_width: int, actual_height: int):
	print("Attempting to play video: ", video_path)
	if not FileAccess.file_exists(video_path):
		push_error("File doesn't exist: ", video_path)
		OS.kill(OS.get_process_id())
		return

	# Create VideoStreamPlayer
	var video_player = VideoStreamPlayer.new()
	video_player.name = "VideoPlayer"
	add_child(video_player)

	var video_stream = load(video_path)
	print("Loaded video stream: ", video_stream)

	if video_stream:
		video_player.stream = video_stream
		video_player.play()

		# Wait for video to load
		await RenderingServer.frame_post_draw
		await get_tree().process_frame

		print("Video dimensions: ", actual_width, "x", actual_height)

		# Define maximum window size
		var max_width = 1280
		var max_height = 720

		# Calculate scale to fit within max dimensions while maintaining aspect ratio
		var scale_x = float(max_width) / float(actual_width)
		var scale_y = float(max_height) / float(actual_height)
		var final_scale = min(min(scale_x, scale_y), 1.0)  # Don't upscale, only downscale

		# Apply scaling to video player
		video_player.scale = Vector2(final_scale, final_scale)

		# Center the video
		video_player.position = Vector2(
			(float(max_width) - float(actual_width) * final_scale) / 2.0,
			(float(max_height) - float(actual_height) * final_scale) / 2.0
		)

		# Create caption with wave animation
		if caption_text != "":
			# Calculate video display dimensions
			var video_display_width = float(actual_width) * final_scale
			var video_display_height = float(actual_height) * final_scale

			# Calculate ideal position (below video)
			var ideal_y = video_player.position.y + video_display_height + 10.0

			# Check if label would be out of bounds (caption bottom > max_height)
			var label_y = ideal_y
			if ideal_y + 150.0 > float(max_height):
				# Position over bottom of video instead
				label_y = float(max_height) - 150.0
				print("Caption out of bounds, positioning over video bottom")

			# Create wave text animation
			var caption_position = Vector2(0.0, label_y)
			var wave_text = create_wave_text(caption_text, caption_position, max_width)

			# Debug information
			print("Video display size: ", video_display_width, "x", video_display_height)
			print("Video position: ", video_player.position)
			print("Caption position: ", caption_position)
			print("Caption text: ", caption_text)

			# Add caption
			await get_tree().create_timer(0.1).timeout
			add_child(wave_text)

		print("Max window size: ", max_width, "x", max_height)
		print("Final scale: ", final_scale)
		print("Scaled size: ", float(actual_width) * final_scale, "x", float(actual_height) * final_scale)
		print("Video position: ", video_player.position)

		video_player.finished.connect(func():
			print("Video finished, exiting")
			OS.kill(OS.get_process_id())
		)
	else:
		push_error("Failed to load video")
		OS.kill(OS.get_process_id())

func play_image(image_path: String, caption_text: String, actual_width: int, actual_height: int):
	print("Attempting to display image: ", image_path)
	if not FileAccess.file_exists(image_path):
		push_error("File doesn't exist: ", image_path)
		OS.kill(OS.get_process_id())
		return

	# Load image
	var image = Image.load_from_file(image_path)
	if not image:
		push_error("Failed to load image")
		OS.kill(OS.get_process_id())
		return

	var texture = ImageTexture.create_from_image(image)
	print("Loaded image texture: ", texture)

	# Create TextureRect to display image
	var texture_rect = TextureRect.new()
	texture_rect.name = "ImageDisplay"
	texture_rect.texture = texture
	texture_rect.expand_mode = TextureRect.EXPAND_IGNORE_SIZE
	texture_rect.stretch_mode = TextureRect.STRETCH_KEEP_ASPECT
	add_child(texture_rect)

	print("Image dimensions: ", actual_width, "x", actual_height)

	# Define maximum window size
	var max_width = 1280
	var max_height = 720

	# Calculate scale to fit within max dimensions while maintaining aspect ratio
	var scale_x = float(max_width) / float(actual_width)
	var scale_y = float(max_height) / float(actual_height)
	var final_scale = min(min(scale_x, scale_y), 1.0)  # Don't upscale, only downscale

	# Calculate display size
	var display_width = float(actual_width) * final_scale
	var display_height = float(actual_height) * final_scale

	# Set texture rect size and position
	texture_rect.size = Vector2(display_width, display_height)
	texture_rect.position = Vector2(
		(float(max_width) - display_width) / 2.0,
		(float(max_height) - display_height) / 2.0
	)

	# Create caption with wave animation
	if caption_text != "":
		# Calculate ideal position (below image)
		var ideal_y = texture_rect.position.y + display_height + 10.0

		# Check if label would be out of bounds (caption bottom > max_height)
		var label_y = ideal_y
		if ideal_y + 150.0 > float(max_height):
			# Position over bottom of image instead
			label_y = float(max_height) - 150.0
			print("Caption out of bounds, positioning over image bottom")

		# Create wave text animation
		label_y = 700
		var caption_position = Vector2(0.0, label_y)
		var wave_text = create_wave_text(caption_text, caption_position, max_width)

		# Debug information
		print("Image display size: ", display_width, "x", display_height)
		print("Image position: ", texture_rect.position)
		print("Caption position: ", caption_position)
		print("Caption text: ", caption_text)

		# Add caption
		await get_tree().create_timer(0.1).timeout
		add_child(wave_text)

	print("Max window size: ", max_width, "x", max_height)
	print("Final scale: ", final_scale)
	print("Scaled size: ", display_width, "x", display_height)
	print("Image position: ", texture_rect.position)

	# Set timer to close after 3 seconds
	await get_tree().create_timer(3.0).timeout
	print("Image display timeout, exiting")
	OS.kill(OS.get_process_id())
