Feature: The controls menu, keyboard layouts, and saving them

  Background:
    Given I am in test room 1
    And I have the smg
    And the target dummy is out of the way

  Scenario: Controls: Escape opens the menu and frees the mouse
    When I press Escape
    Then the menu is open
    And the mouse is free

  Scenario: Controls: Escape again closes it and takes the mouse back
    Given the menu is open
    When I press Escape
    Then the menu is closed
    And the mouse is captured

  Scenario: Controls: you can't fire with the menu open
    Given the menu is open
    When I hold fire for 0.5 seconds
    Then the gun fired between 0 and 0 shots

  Scenario: Controls: you don't walk with the menu open
    Given the menu is open
    When I hold W
    Then I have not moved

  Scenario: Controls: the default layout is QWERTY, W A S D
    When I hold W
    Then I have moved forward

  Scenario: Controls: S goes back on QWERTY
    When I hold S
    Then I have moved back

  Scenario: Controls: D goes right on QWERTY
    When I hold D
    Then I have moved right

  Scenario: Controls: Colemak Mod-DH moves forward on W
    Given my controls are Colemak Mod-DH
    When I hold W
    Then I have moved forward

  Scenario: Controls: Colemak Mod-DH goes back on R, where QWERTY has S
    Given my controls are Colemak Mod-DH
    When I hold R
    Then I have moved back

  Scenario: Controls: Colemak Mod-DH goes right on S, where QWERTY has D
    Given my controls are Colemak Mod-DH
    When I hold S
    Then I have moved right

  Scenario: Controls: and D does nothing in Colemak Mod-DH movement
    Given my controls are Colemak Mod-DH
    When I hold D
    Then I have not moved

  Scenario: Controls: Colemak Mod-DH reloads on P
    Given my controls are Colemak Mod-DH
    And I fire 3 single shots
    When I press P
    Then the gun is reloading

  Scenario: Controls: and R doesn't reload there
    Given my controls are Colemak Mod-DH
    And I fire 3 single shots
    When I press R
    Then the gun is not reloading

  Scenario: Controls: choosing a preset in the menu switches the layout
    Given the menu is open
    When I choose Colemak Mod-DH in the menu
    And I press Escape
    And I hold R
    Then I have moved back

  Scenario: Controls: you can rebind a single key
    Given the menu is open
    When I choose to rebind reload
    Then the menu is waiting for a key for reload
    When I press G
    Then reload is bound to g
    And the menu is not waiting for a key
    When I press Escape
    And I fire 3 single shots
    And I press G
    Then the gun is reloading

  Scenario: Controls: rebinding to a key in use swaps the two
    Given the menu is open
    When I choose to rebind reload
    And I press C
    Then reload is bound to c
    And crouch is bound to r

  Scenario: Controls: Escape gives up on a rebind without changing anything
    Given the menu is open
    When I choose to rebind reload
    And I press Escape
    Then the menu is not waiting for a key
    And the menu is open
    And reload is bound to r

  Scenario: Controls: saving writes the layout to the settings file
    Given my controls are Colemak Mod-DH
    When I save my controls
    Then the saved controls say "preset = colemak_mod_dh"

  Scenario: Controls: saved changes to single keys are kept too
    Given the menu is open
    When I choose to rebind reload
    And I press G
    And I save my controls
    Then the saved controls say "reload = g"
