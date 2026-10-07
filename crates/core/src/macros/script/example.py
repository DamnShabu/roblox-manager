# An example to copy from: walks forward, jumps, and clicks a picked image when it shows.
#
# Every .py file in this folder is an advanced macro. Select accounts in
# Roblox Manager and press its Run in the Advanced tab: it runs once for each
# of them, into that account's macro-ready window. Stop ends it.
#
# The rbxmgr module is how a script plays: keys, clicks and looks at the
# window all go through the manager, never into the Roblox client.
#
#   tap("e"), tap("ctrl+w"), hold("w", secs=2), key_down/key_up("shift")
#   type_text("hello"), click(400, 300), click(button="mouse2")
#   move_to(x, y), move(dx, dy) to turn the camera, scroll(-3)
#   wait(secs)
#   find("coin") -> (x, y) of its corner or None, find_center("coin") its middle,
#   for an image picked in the Macros tab
#   wait_for("coin", timeout=10), pixel(x, y), color_is(x, y, (255, 0, 0))
#   frame() -> a copy of the window, frame().save("shot.png")
#   images(), running(), account, user_id
#
# print() shows up in the manager's activity log.

import rbxmgr as rb

print(f"playing on {rb.account}")

rounds = 0
while True:
    rb.hold("w", secs=1.5)
    rb.tap("space")

    # A picked image is the easy way to react to the game. Pick one with a
    # plain macro's When step; it is saved under a name such as image1.
    names = rb.images()
    if names:
        at = rb.find_center(names[0])
        if at:
            print(f"saw {names[0]} at {at}, clicking it")
            rb.click(*at)

    rounds += 1
    if rounds % 10 == 0:
        print(f"{rounds} rounds done")
    rb.wait(0.5)
