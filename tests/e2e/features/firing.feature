Feature: Firing the gun

  Background:
    Given I am in test room 1
    And I have the smg

  Scenario: Firing plays the shot sound on the audio device
    Given the shot sound is loaded
    When I fire once
    Then the shot sound plays on the audio device

  Scenario: Firing with the cursor released does nothing
    Given the cursor is released
    When I hold fire for 0.5 seconds
    Then no shot is fired

  Scenario: Holding fire is rate limited
    When I hold fire for 1 second
    Then the gun fired between 7 and 9 shots

  Scenario: An aimed shot hits the target dummy
    Given I right click
    And the gun is on its sights
    When I fire once
    Then the target dummy has taken 25 damage

  Scenario: The target dummy respawns after being destroyed
    Given I right click
    And the gun is on its sights
    When I fire single shots until the target dummy is down
    Then the target dummy respawns within 3 seconds

  Scenario: Firing kicks the view upward
    When I fire once
    Then the view has kicked upward

  Scenario: A burst climbs, and then settles most of the way back
    When I hold fire for 1 second
    Then the view has climbed noticeably
    And the view has come most of the way back down

  Scenario: A shot from the hip is inaccurate
    When I fire 12 single shots
    Then the worst shot strayed more than 0.8 degrees

  Scenario: Shots from the sights are accurate
    Given I right click
    And the gun is on its sights
    When I fire 12 single shots
    Then no shot strayed more than 0.4 degrees

  Scenario: Firing shows a muzzle flash that goes away again
    When I fire once
    Then the muzzle flash shows
    And the muzzle flash is gone
